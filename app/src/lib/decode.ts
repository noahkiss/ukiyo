import type { Catalog_Serialize, Entry_Serialize, PropRow, Value } from "../bindings";

export type Catalog = Catalog_Serialize;
export type Entry = Entry_Serialize;

export const hex = (code: number) => "0x" + code.toString(16).padStart(4, "0");

/** The key a catalog labels a value by: decimal for numbers, the text for strings. */
export function valueKey(v: Value | null): string {
  if (!v) return "";
  switch (v.kind) {
    case "int":
      return String(v.v);
    case "str":
      return v.v;
    case "array":
      return v.v.join(",");
    case "raw":
      return "0x" + v.v;
  }
}

/** The raw value for the tooltip: decimal and hex for numbers. */
export function rawText(v: Value | null): string {
  if (!v) return "no value";
  if (v.kind === "int") {
    const u = v.v < 0 ? v.v + 2 ** 32 : v.v;
    return `${v.v} (0x${u.toString(16)})`;
  }
  if (v.kind === "str") return JSON.stringify(v.v);
  return valueKey(v);
}

export function entry(cat: Catalog | null, code: number): Entry | undefined {
  return cat?.properties[hex(code)];
}

/** A value as the camera would show it: its label, else scaled with its unit, else raw. */
export function display(e: Entry | undefined, v: Value | null): string {
  if (!v) return "—";
  const k = valueKey(v);
  const label = e?.values?.[k];
  if (label) return label;
  if (v.kind === "int" && e?.scale) {
    const n = v.v / e.scale;
    const s = Number.isInteger(n) ? String(n) : n.toFixed(2).replace(/0+$/, "");
    return e.unit === "f/" ? `f/${s}` : `${s}${e.unit ? " " + e.unit : ""}`;
  }
  if (v.kind === "str") return v.v === "" ? '""' : v.v;
  return k;
}

export function formValues(row: PropRow): Value[] {
  return row.form.form === "enum" ? row.form.values : [];
}

export const TYPE_NAMES: Record<number, string> = {
  0: "undefined",
  1: "int8",
  2: "uint8",
  3: "int16",
  4: "uint16",
  5: "int32",
  6: "uint32",
  7: "int64",
  8: "uint64",
  0xffff: "string",
};

export function time(ms: number | null): string {
  if (ms == null) return "";
  return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit", hour12: false });
}
