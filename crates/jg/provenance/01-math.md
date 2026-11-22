# Group 1: Clamp, Lerp, Sign

Written 2026-11-22. Source: the plugin's GECK Wiki pages, read in the saved copies of 2026-11-21
(geckwiki.com/index.php/Clamp, last edited 9 April 2026; /Lerp; /Sign). Licence: the pages state
none; facts only, no text or example code copied. Opcodes: the commands' places in the plugin's
public registration list (names and order only; `Clamp` 0x3155, `Lerp` 0x3157, `Sign` 0x315A).

| Command | What the page gives | What it leaves open | What the code does |
|---|---|---|---|
| `Clamp value min max` | "Clamps the value to a specified range"; three examples: (5,10,20) is 10, (75,100,50) is 100, (1000,500,50) is 50 | the answer for a range with min above max | the order the examples force: below min gives min, else above max gives max |
| `Lerp v0 v1 t` | "an interpolation between two inputs (v0, v1) for a parameter (t) in the closed unit interval [0, 1]" | t outside the interval; how the sum is rounded | `v0*(1-t) + v1*t` in `float`, so the ends are exact; t outside goes on along the line |
| `Sign value` | "returns sign of a value", signature `(1, -1:int)`, example 5 is 1 | the answer for 0 | 1, -1, and 0 for 0 and NaN |

Not built from the same list: `ApplyEasing` (ShowOff): its page points to an outside page for the
formulas and gives none.
