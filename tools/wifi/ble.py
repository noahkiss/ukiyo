"""Read-only Bluetooth LE survey of a Fujifilm camera with bleak.

Scans, then connects and lists GATT services, characteristics and descriptors. It reads only
characteristics that allow reads and are not on DENY. It never writes, subscribes or pairs.

UUIDs and roles: petabyt/libfuji lib/bluetooth.c (from gkoh/furble). Company ID: furble Fujifilm.h.
"""
import asyncio

FUJI_COMPANY_ID = 0x04D8

# Reads that may carry secrets or start pairing on macOS. Logged as present, never read.
DENY = {
    "bf6dc9cf-3606-4ec9-a4c8-d77576e93ea4": "camera Wi-Fi SSID",
    "e809256a-915c-4967-92e8-53b7d4cad213": "camera Wi-Fi password",
    "f557d96b-8284-4667-8793-b971c1deca2a": "secure pairing status",
    "aba356eb-9633-4e60-b73f-f52516dbd671": "pairing",
    "85b9163e-62d1-49ff-a6f5-054b4630d4a1": "client identity",
    "ac0c799a-fa6c-4df5-bbc5-bb95cce7e6ea": "backup file data",
}
NAMES = {
    "91f1de68-dff6-466e-8b65-ff13b0f16fb8": "pairing service (legacy)",
    "123d8f06-62a1-4935-9322-833c531ee225": "secure pairing service",
    "4e941240-d01d-46b9-a5ea-67636806830b": "Wi-Fi hand-off and notifications",
    "6514eb81-4e8f-458d-aa2a-e691336cdfac": "shutter / Wi-Fi trigger",
    "600655e6-3637-42f1-8fb2-44efc5c63b13": "Wi-Fi trigger (write 04 00)",
    "7fcf49c6-4ff0-4777-a03d-1a79166af7a8": "shutter",
    "af854c2e-b214-458e-97e2-912c4ecf2cb8": "file service: backup, equipment status",
    "c922ac69-9480-4348-8f4b-9ee29bc30d1d": "backup file name",
    "4c0020fe-f3b6-40de-acc9-77d129067b14": "configuration / indications",
    "3b46ec2b-48ba-41fd-b1b8-ed860b60d22b": "geotag",
    "15ca59fe-620c-464d-a987-223fab660cde": "lens",
    "68887a7a-10a7-4a4a-aadd-6f9db1abaa0e": "lens name",
    "621a98d8-6314-4c3e-a683-eea1d6166606": "lens firmware",
    "117c4142-edd4-4c77-8696-dd18eebb770a": "XApp service",
    "e872b11f-d526-4ae1-9bb4-89a99d48fa59": "time sync",
    "b9bfd37f-ccad-4d36-a1ee-018e792b3edf": "time",
    **{k: "denied: " + v for k, v in DENY.items()},
}


def is_fuji(name, mfg):
    return FUJI_COMPANY_ID in (mfg or {}) or bool(name and ("FUJIFILM" in name.upper() or name.upper().startswith("X-")))


def advert_record(addr, name, adv):
    mfg = {k: bytes(v) for k, v in (adv.manufacturer_data or {}).items()}
    rec = {"address": addr, "name": name or adv.local_name, "rssi": adv.rssi, "tx_power": adv.tx_power,
           "service_uuids": list(adv.service_uuids or []),
           "service_data": {k: bytes(v).hex() for k, v in (adv.service_data or {}).items()}}
    fuji = mfg.get(FUJI_COMPANY_ID)
    if fuji is not None:
        # Byte 0 is the type; 0x02 carries a 4-byte pairing token (libfuji bluetooth.c).
        rec["fuji_mfg"] = {"type": fuji[0] if fuji else None, "len": len(fuji),
                           "token": fuji[1:5].hex() if len(fuji) >= 5 else None}
    rec["other_mfg_ids"] = [f"{k:#06x}" for k in mfg if k != FUJI_COMPANY_ID]
    return rec


async def _scan(seconds):
    from bleak import BleakScanner
    found = await BleakScanner.discover(timeout=seconds, return_adv=True)
    total = len(found)
    fuji = []
    for addr, (dev, adv) in found.items():
        if is_fuji(dev.name or adv.local_name, adv.manufacturer_data):
            fuji.append((dev, advert_record(addr, dev.name, adv)))
    return total, fuji


async def _survey(device, read, timeout):
    from bleak import BleakClient
    out = {"services": [], "connected": False}
    async with BleakClient(device, timeout=timeout) as c:
        out["connected"] = c.is_connected
        try:
            out["mtu"] = c.mtu_size
        except Exception:  # noqa: BLE001 - not every backend has it
            pass
        for svc in c.services:
            s = {"uuid": svc.uuid, "role": NAMES.get(svc.uuid, svc.description), "characteristics": []}
            for ch in svc.characteristics:
                rec = {"uuid": ch.uuid, "handle": ch.handle, "properties": list(ch.properties),
                       "role": NAMES.get(ch.uuid, ch.description),
                       "descriptors": [d.uuid for d in ch.descriptors]}
                if ch.uuid in DENY:
                    rec["read"] = "skipped: " + DENY[ch.uuid]
                elif read and "read" in ch.properties:
                    try:
                        val = await asyncio.wait_for(c.read_gatt_char(ch), 6)
                        rec["value"] = bytes(val)
                    except Exception as e:  # noqa: BLE001 - record any failure
                        rec["read"] = f"error: {type(e).__name__}: {e}"[:160]
                s["characteristics"].append(rec)
            out["services"].append(s)
    return out


# One loop for every call: bleak's CoreBluetooth manager binds to the loop of the first scan, and
# a device found there cannot be used from a second asyncio.run (RuntimeError, "attached to a
# different loop").
_loop = None


def _run(coro):
    global _loop
    if _loop is None:
        _loop = asyncio.new_event_loop()
    return _loop.run_until_complete(coro)


def scan(seconds):
    return _run(_scan(seconds))


def survey(device, read=True, timeout=20.0):
    return _run(_survey(device, read, timeout))
