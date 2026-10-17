# Menu sets across bodies

This page compares the menu sets in `catalog/menus/`. The sets come from Fujifilm's English HTML
owner's manuals; `docs/catalog.md` (Menu sets) gives the format and the build command. Every
fact here comes from those files. A dash means the manual's menu list has no such item.

## Bodies covered

| Generation | Body | Manual firmware | Layout | Items |
|---|---|---|---|---|
| X-Trans V (X-Processor 5) | X-T5 | 4.30 | menu list | 242 |
| | X-T50 | 1.30 | menu list | 245 |
| | X-H2 | 5.30 | menu list | 241 |
| | X-H2S (stacked 26 MP, CMOS 5 HS) | 7.30 | menu list | 238 |
| | X-E5 | 1.10 | menu list | 247 |
| | X100VI | 1.30 | menu list | 242 |
| X-Trans IV + X-Processor 5 | X-S20 | 3.30 | menu list | 244 |
| | X-M5 | 1.30 | menu list | 250 |
| X-Trans IV (X-Processor 4) | X-T4 | 2.00 | menu list | 221 |
| | X-S10 | 3.00 | menu list | 221 |
| | X-E4 | 2.00 | menu list | 218 |
| | X-Pro3 | 2.00 | menu list | 186 |
| | X100V | 3.00 | menu list | 184 |
| | X-T30 II | 2.00 | menu list | 215 |
| | X-T3 | 5.00 | menu list | 183 |
| | X-T30 | 2.00 | menu list | 170 |
| X-Trans III (X-Processor Pro) | X-H1 | 2.00 | menu list | 170 |
| | X-Pro2 | 5.00 | section index | 127 |
| | X-T2 | 4.20 | menu list | 138 |
| | X-E3 | 1.20 | menu list | 125 |
| | X100F | 2.10 | section index | 115 |

All 21 requested bodies fetched. "Items" counts every top-level menu item in all tabs, photo
and movie menus separately. The X-Pro2 and X100F manuals have no Menu List page, so their pages
are unknown and tab membership comes from the factory-defaults tables. Those two are also the
only manuals that print factory defaults.

## IMAGE QUALITY SETTING, photo menus

The recipe-relevant tab. Cells give the option count, `R` for a range, `y` for an item with
sub-items or with options shown only as pictures.

| Item | T5 | T50 | H2 | H2S | E5 | 100VI | S20 | M5 | T4 | S10 | E4 | Pro3 | 100V | T30 II | T3 | T30 | H1 | Pro2 | T2 | E3 | 100F |
|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|---|
| FILM SIMULATION | 20 | 21 | 20 | 20 | 20 | 20 | 21 | 21 | 18 | 19 | 18 | 17 | 17 | 19 | 16 | 16 | 16 | 15 | 15 | 15 | 15 |
| SELECT JPEG/HEIF | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | - | - | - | - | - | - | - | - | - | - | - | - | - |
| RAW RECORDING | 3 | 3 | 3 | 3 | 3 | y | 3 | 3 | 3 | 3 | 3 | 2 | 2 | 3 | 2 | 2 | 2 | 2 | 2 | 2 | 2 |
| MONOCHROMATIC COLOR | y | y | y | y | y | y | y | y | y | y | y | y | y | y | - | - | - | - | - | - | - |
| B & W ADJ. (Warm/Cool) | - | - | - | - | - | - | - | - | - | - | - | - | - | - | R | R | - | - | - | - | - |
| GRAIN EFFECT ROUGHNESS | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | y | 3 | 3 | y |
| GRAIN EFFECT SIZE | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | - | - | - | - | - | - | - |
| COLOR CHROME EFFECT | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | - | - | - | - | - |
| COLOR CHROME FX BLUE | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | 3 | - | - | - | - | - | - | - |
| SMOOTH SKIN EFFECT | 3 | 3 | 3 | - | 3 | 3 | - | - | - | - | - | - | - | - | - | - | - | - | - | - | - |
| PORTRAIT ENHANCER LV | - | - | - | - | - | - | - | 4 | - | - | - | - | - | - | - | - | - | - | - | - | - |
| WHITE BALANCE | 14 | 14 | 14 | 14 | 14 | 14 | 14 | 14 | 14 | 14 | 14 | 12 | 12 | 14 | 12 | 12 | 12 | 12 | 12 | 12 | 12 |
| D RANGE PRIORITY | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | 4 | - | - | - | - |
| TONE CURVE (HIGHLIGHTS, SHADOWS) | R | R | R | R | R | R | R | R | R | R | R | R | R | R | - | - | - | - | - | - | - |
| HIGHLIGHT TONE, SHADOW TONE | - | - | - | - | - | - | - | - | - | - | - | - | - | - | R | R | R | y | R | R | y |
| HIGH ISO NR / NOISE REDUCTION | R | R | R | R | R | R | R | R | R | R | R | R* | R* | R | R* | R* | R* | y* | R* | R* | y* |
| CLARITY | R | R | R | R | R | R | R | R | R | R | R | R | R | R | - | - | - | - | - | - | - |
| LENS MODULATION OPTIMIZER | 2 | 2 | 2 | 2 | 2 | - | 2 | 2 | 2 | 2 | 2 | 2 | - | 2 | 2 | 2 | 2 | y | 2 | 2 | - |
| SELECT CUSTOM SETTING | y | y | - | - | y | y | - | - | y | - | y | y | y | y | y | y | y | y | y | y | y |
| CUSTOM MODE SETTING | - | - | y | y | - | - | y | y | - | - | - | - | - | - | - | - | - | - | - | - | - |
| AUTO UPDATE CUSTOM SETTING | 2 | 2 | 2 | 2 | 2 | 2 | 2 | 2 | - | - | 2 | - | - | 2 | - | - | - | - | - | - | - |
| FILM SIMULATION DIAL SETTING | - | y | - | - | y | - | - | y | - | - | - | - | - | - | - | - | - | - | - | - | - |

`*` The item is called NOISE REDUCTION on that body.

Ranges where the manuals give them:

- COLOR, SHARPNESS and HIGH ISO NR / NOISE REDUCTION: -4 to +4 on every body.
- CLARITY: -5 to +5.
- TONE CURVE HIGHLIGHTS and SHADOWS: -2 to +4. The X-Pro3 and X100V print one -2 to +4 scale for
  TONE CURVE. HIGHLIGHT TONE and SHADOW TONE on X-Trans III and the X-T3/X-T30: -2 to +4.
- B & W ADJ. (Warm/Cool): -9 to +9.
- Color temperature: 2500 K to 10000 K, where the manual states it (X-Trans V, X-S20, X-M5,
  X-S10, X-E4, X-T30 II).
- The manuals give no step for TONE CURVE. They show MONOCHROMATIC COLOR and the
  white-balance shift only as screenshots, so the menu sets carry no range for them.

### Film simulations by body

| Film simulation | X-Trans V | X-S20, X-M5 | X-T4, X-E4 | X-S10, X-T30 II | X-Pro3, X100V | X-T3, X-T30, X-H1 | X-Pro2, X-T2, X-E3, X100F |
|---|---|---|---|---|---|---|---|
| AUTO | X-T50 only | yes | - | yes | - | - | - |
| PROVIA/STANDARD, Velvia/VIVID, ASTIA/SOFT, CLASSIC CHROME | yes | yes | yes | yes | yes | yes | yes |
| PRO Neg. Hi, PRO Neg. Std | yes | yes | yes | yes | yes | yes | yes |
| ACROS (+Ye, +R, +G FILTER) | yes | yes | yes | yes | yes | yes | yes |
| MONOCHROME (+Ye, +R, +G FILTER), SEPIA | yes | yes | yes | yes | yes | yes | yes |
| ETERNA/CINEMA | yes | yes | yes | yes | yes | yes | - |
| CLASSIC Neg. | yes | yes | yes | yes | yes | - | - |
| ETERNA BLEACH BYPASS | yes | yes | yes | yes | - | - | - |
| REALA ACE | yes | yes | - | - | - | - | - |
| NOSTALGIC Neg. | yes | yes | - | - | - | - | - |

## The 10 most useful differences

1. **Film simulations follow the processor, not the sensor.** REALA ACE and NOSTALGIC Neg.
   appear on every X-Processor 5 body, including the X-Trans IV X-S20 and X-M5. No X-Processor
   4 manual lists them.
2. **ETERNA BLEACH BYPASS is missing on the X-Pro3 and X100V**, though the later X-Trans IV
   bodies (X-T4, X-S10, X-E4, X-T30 II) have it. CLASSIC Neg. is missing on the X-T3, X-T30
   and X-H1.
3. **SMOOTH SKIN EFFECT is a 40 MP X-Trans 5 HR item.** The X-T5, X-T50, X-H2, X-E5 and
   X100VI have it; the X-H2S, X-S20 and X-M5 do not. The X-M5 has PORTRAIT ENHANCER LV
   (WEAK, MEDIUM, STRONG, OFF) instead.
4. **COLOR CHROME FX BLUE, CLARITY, GRAIN EFFECT SIZE and MONOCHROMATIC COLOR arrive together**
   with the X-Pro3 generation. The X-T3 and X-T30 have COLOR CHROME EFFECT but none of these;
   their mono tint item is B & W ADJ. (Warm/Cool), one axis only.
5. **X-Trans III has no COLOR CHROME EFFECT** in these manuals (X-H1, X-Pro2, X-T2, X-E3,
   X100F), and grain has STRONG, WEAK and OFF only.
6. **Tone controls changed shape twice.** X-Trans III and the X-T3/X-T30 have separate
   HIGHLIGHT TONE and SHADOW TONE items. The X-Pro3 and X100V fold them into TONE CURVE. From
   the X-T4 on, TONE CURVE has HIGHLIGHTS and SHADOWS sub-items.
7. **Noise reduction was renamed.** It is NOISE REDUCTION up to the X-Pro3 and X100V, and HIGH
   ISO NR from the X-T4 on. A recipe that says "NR" maps to either.
8. **Custom-slot access differs.** The X-H2, X-H2S, X-S20 and X-M5 put C1-C7 on the mode dial
   (CUSTOM MODE SETTING) and have no SELECT CUSTOM SETTING. The X-S10 has no SELECT CUSTOM
   SETTING either. AUTO UPDATE CUSTOM SETTING exists on
   X-Processor 5 bodies plus the X-E4 and X-T30 II.
9. **Some bodies have a film simulation dial.** The X-T50, X-E5 and X-M5 have FILM SIMULATION
   DIAL SETTING for the FS1-FS3 positions. On the X-E5 a position can hold a FILM SIMULATION
   or an FS RECIPE.
10. **White balance gains two AUTO variants.** WHITE PRIORITY and AMBIENCE PRIORITY appear from
    the X-T4 on (and on the X-S10, X-E4, X-T30 II); earlier bodies have AUTO only. HEIF output
    (SELECT JPEG/HEIF) and the third RAW option, COMPRESSED, also mark the line: HEIF is
    X-Processor 5 only, COMPRESSED RAW starts with the X-T4.

Other shooting-menu items limited to a few bodies:

| Item | Bodies |
|---|---|
| SUBJECT DETECTION SETTING, FLICKERLESS S.S. SETTING, HIGH SPEED REC, MEDIA REC SETTING | X-Processor 5 bodies |
| WRAP FOCUS POINT, INTERVAL PRIORITY MODE | X-T5, X-T50, X-E5, X100VI, X-S20, X-M5 |
| COOLING FAN SETTING | X-H2, X-H2S, X-S20, X-M5 |
| DIGITAL TELE-CONV. | X-T5, X-T50, X-H2, X-E5, X100VI, X100V, X100F |
| CONVERSION LENS, ND FILTER, Built-In Flash | X100VI, X100V, X100F |
| MULTI EXPOSURE (named item) | X-T5, X-T4, X-H1 |

## X-T5 crosswalk to `catalog/x-t5.json`

Menu items from `catalog/menus/x-t5.json` matched to named properties in `catalog/x-t5.json`,
only where the catalog's names make the match clear. The live code is what tether mode reads.
The C1-C7 slot code is the RAW CONV. mode copy that the custom-setting banks use. Confidence is
the catalog's.

| Menu item (X-T5) | Live code | C1-C7 slot code (rawconv) |
|---|---|---|
| IMAGE QUALITY SETTING > IMAGE SIZE | `0x5003` ImageSize (confirmed) | `0xd18e` PresetImageSize (likely) |
| IMAGE QUALITY SETTING > IMAGE QUALITY | `0xd018` Quality (likely) | `0xd18f` PresetImageQuality (likely) |
| IMAGE QUALITY SETTING > RAW RECORDING | `0xd022` RawCompression (likely) | |
| IMAGE QUALITY SETTING > SELECT JPEG/HEIF | `0xd1b2` ImageFormat (confirmed) | |
| IMAGE QUALITY SETTING > FILM SIMULATION | `0xd001` FilmSimulation (confirmed) | `0xd192` PresetFilmSimulation (likely) |
| IMAGE QUALITY SETTING > MONOCHROMATIC COLOR | `0xd104` MonochromaticColorWC, `0xd031` MonochromaticColorMG (confirmed) | `0xd193` PresetMonoWC, `0xd194` PresetMonoMG (likely) |
| IMAGE QUALITY SETTING > GRAIN EFFECT | `0xd023` GrainEffect (confirmed) | `0xd195` PresetGrainEffect (likely) |
| IMAGE QUALITY SETTING > COLOR CHROME EFFECT | `0xd029` ColorChromeEffect (confirmed) | `0xd196` PresetColorChrome (likely) |
| IMAGE QUALITY SETTING > COLOR CHROME FX BLUE | `0xd030` ColorChromeFXBlue (confirmed) | `0xd197` PresetColorChromeFxBlue (likely) |
| IMAGE QUALITY SETTING > SMOOTH SKIN EFFECT | `0xd189` SmoothSkinEffect (confirmed) | `0xd198` PresetSmoothSkin (likely) |
| IMAGE QUALITY SETTING > WHITE BALANCE | `0x5005` WhiteBalance (confirmed) | `0xd199` PresetWhiteBalance (likely) |
| WHITE BALANCE fine-tuning (R/B shift) | `0xd00b` WhiteBalanceShiftR, `0xd00c` WhiteBalanceShiftB (confirmed) | `0xd19a` PresetWBShiftR, `0xd19b` PresetWBShiftB (likely) |
| WHITE BALANCE > Color Temperature | `0xd017` ColorTemperature (confirmed) | `0xd19c` PresetColorTemp (likely) |
| IMAGE QUALITY SETTING > DYNAMIC RANGE | `0xd007` DRangeMode (likely) | `0xd190` PresetDynamicRange (likely) |
| IMAGE QUALITY SETTING > D RANGE PRIORITY | `0xd02e` DRangePriority (confirmed) | |
| TONE CURVE > HIGHLIGHTS | `0xd320` HighLightTone (confirmed) | `0xd19d` PresetHighlightTone (likely) |
| TONE CURVE > SHADOWS | `0xd321` ShadowTone (likely) | `0xd19e` PresetShadowTone (likely) |
| IMAGE QUALITY SETTING > COLOR | `0xd008` Color (confirmed) | `0xd19f` PresetColor (likely) |
| IMAGE QUALITY SETTING > SHARPNESS | `0x5015` Sharpness (confirmed) | `0xd1a0` PresetSharpness (likely) |
| IMAGE QUALITY SETTING > HIGH ISO NR | `0xd01c` NoiseReduction (confirmed) | `0xd1a1` PresetHighIsoNR (likely) |
| IMAGE QUALITY SETTING > CLARITY | `0xd032` Clarity (confirmed) | `0xd1a2` PresetClarity (likely) |
| IMAGE QUALITY SETTING > LONG EXPOSURE NR | `0xd322` LongExposureNR (likely) | `0xd1a3` PresetLongExpNR (likely) |
| IMAGE QUALITY SETTING > LENS MODULATION OPTIMIZER | `0xd34d` LMOMode (likely) | |
| IMAGE QUALITY SETTING > COLOR SPACE | `0xd00a` ColorSpace (likely) | `0xd1a4` PresetColorSpace (likely) |
| IMAGE QUALITY SETTING > SELECT CUSTOM SETTING | `0xd18c` PresetSlot (confirmed) | |
| IMAGE QUALITY SETTING > EDIT/SAVE CUSTOM SETTING | `0xd18c` PresetSlot, `0xd18d` PresetName (confirmed) | |
| AF/MF SETTING > FOCUS AREA | `0xd347` FocusPoint, `0xd372` FocusArea1, `0xd374` FocusArea3, `0xd395` FocusArea4 (likely) | |
| AF/MF SETTING > AF-C CUSTOM SETTINGS | `0xd232` AfcCustomSet, `0xd233` AfcCustomValues (likely) | |
| AF/MF SETTING > NUMBER OF FOCUS POINTS | `0xd025` FocusPoints (confirmed) | |
| AF/MF SETTING > PRE-AF | `0xd34b` PreAFMode (confirmed) | |
| AF/MF SETTING > AF ILLUMINATOR | `0xd112` AFIlluminator (likely) | |
| AF/MF SETTING > FACE/EYE DETECTION SETTING | `0xd020` FaceDetectionMode, `0xd024` SetEyeAFMode (likely) | |
| AF/MF SETTING > AF+MF | `0xd323` FullTimeManualFocus (likely) | |
| AF/MF SETTING > MF ASSIST | `0xd026` MFAssistMode (likely) | |
| AF/MF SETTING > FOCUS CHECK | `0xd35e` FocusCheckMode (likely) | |
| AF/MF SETTING > INTERLOCK SPOT AE & FOCUS AREA | `0xd027` InterlockAEAFArea (likely) | |
| AF/MF SETTING > INSTANT AF SETTING | `0xd34a` InstantAFMode (likely) | |
| AF/MF SETTING > RELEASE/FOCUS PRIORITY | `0xd10a` ReleaseFocusPriorityAFS, `0xd10b` ReleaseFocusPriorityAFC (likely) | |
| SHOOTING SETTING > SELF-TIMER | `0x5012` CaptureDelay (confirmed) | |
| SHOOTING SETTING > PHOTOMETRY | `0x500b` ExposureMeteringMode (confirmed) | |
| SHOOTING SETTING > IS MODE | `0xd351` ISMode (likely) | |
| SHOOTING SETTING > DIGITAL TELE-CONV. | `0xd1b3` DigitalTeleConv (likely) | |
| USER SETTING > DATE/TIME | `0x5011` DateTime (confirmed) | |
| USER SETTING > TIME DIFFERENCE | `0xd158` TimeDifference1, `0xd159` TimeDifference2 (likely) | |
| USER SETTING > LANG. | `0xd15a` Language (likely) | |
| USER SETTING > RESET | `0xd17f` ResetSetting (likely) | |
| SCREEN SETTING > PREVIEW EXP./WB IN MANUAL MODE | `0xd359` ExposurePreview (likely) | |
| SCREEN SETTING > FRAMING GUIDELINE | `0xd106` FrameGuideMode, `0xd375` FrameGuideGridInfo1, `0xd376` FrameGuideGridInfo2 (likely) | |
| SCREEN SETTING > FOCUS SCALE UNITS | `0xd35f` FocusScaleUnit (likely) | |
| SCREEN SETTING > DISP. CUSTOM SETTING | `0xd36e` CustomDispInfo (likely) | |
| BUTTON/DIAL SETTING > COMMAND DIAL SETTING | `0xd028` CommandDialMode (likely) | |
| BUTTON/DIAL SETTING > LENS ZOOM/FOCUS SETTING | `0xd1bf` ConstantSpeedFocus, `0xd1c0` ConstantSpeedZoom (likely) | |
| BUTTON/DIAL SETTING > AE/AF-LOCK MODE | `0xd34e` AeAfLockMode (likely) | |
| BUTTON/DIAL SETTING > LOCK | `0xd136` FunctionLock, `0xd36f` FunctionLockCategory1, `0xd370` FunctionLockCategory2 (likely) | |
| POWER MANAGEMENT > AUTO POWER OFF | `0xd381` AutoPowerOff (confirmed) | |
| SAVE DATA SETTING > FRAME NO. | `0xd15b` FrameNumberSequence, `0xd16d` FrameNumber (likely) | |
| SAVE DATA SETTING > EDIT FILE NAME | `0xd365` FileNamePrefix1, `0xd366` FileNamePrefix2 (likely) | |
| SAVE DATA SETTING > COPYRIGHT INFO | `0xd215` Copyright, `0xd216` Copyright2 (likely), `0xd237` CopyrightAuthor (guess) | |
| NETWORK/USB SETTING > CONNECTION MODE | `0xd16e` UsbConnectionMode (likely) | |

The X-T5 menu set lists 242 items; 59 rows above cover the ones the catalog names. Items with no
named code yet include SHUTTER TYPE, AF MODE, SUBJECT DETECTION SETTING and the
whole movie menu. `local/camera/x-t5/menu-map.md` (not in git) holds candidate codes for some of
them.
