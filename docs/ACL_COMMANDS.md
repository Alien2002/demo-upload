# ACL command reference

Commands are typed in the **ACL>** bar above the grid and run on the table that is open (an imported table or a session result). Keywords are case-insensitive.

```
COUNT IF Amount > 1000
EXTRACT FIELDS Account Amount IF Amount > 1000      -> session result
SORT ON Amount D                                    -> runs on that result
TOTAL Amount                                        -> TOTAL1
```

---

## Scope clauses (every command)

| Clause | Meaning |
|---|---|
| `IF condition` | Only records where the condition is true. |
| `WHILE condition` | Process records in order and **stop at the first record** where the condition isn't true (a blank result also stops). |
| `FIRST n` | Process the first *n* records. |
| `NEXT n` | Process *n* records starting at the **selected grid row** (from the first row if none is selected, or if the view is filtered/sorted/searched). |

Clauses can appear in any order, each at most once; `FIRST` and `NEXT` can't be combined. The scope (`FIRST`/`NEXT`/`WHILE`) picks the records that are processed; `IF` then selects among them:
`COUNT FIRST 100 IF Posted` counts posted records **among the first 100**.

---

## Commands

### COUNT
```
COUNT [IF …] [WHILE …] [FIRST n | NEXT n]
```
Number of records → **COUNT1**.

### TOTAL
```
TOTAL [FIELDS] number_field … [IF …] [WHILE …] [FIRST n | NEXT n]
```
Sum of each field → **TOTAL1**, **TOTAL2**, … in order. Fields must be numbers (expressions allowed: `TOTAL Amount * 1.18`).

### EXTRACT
```
EXTRACT [RECORD | FIELDS item [AS "name"] …] [TO "table"] [IF …] [WHILE …] [FIRST n | NEXT n]
```
Copies records to a new table. `RECORD` (default) keeps every column; `FIELDS` picks columns or computes new ones:
`EXTRACT FIELDS Account, Amount * 1.18 AS "Gross" IF Amount > 1000`

### SORT
```
SORT ON key [D] [key [D] …] [TO "table"] [IF …] [WHILE …] [FIRST n | NEXT n]
```
`D` after a key sorts it descending. Records with equal keys keep their original order. Blanks sort last.

### SUMMARIZE
```
SUMMARIZE ON key … [SUBTOTAL number_field …] [OTHER field …] [TO "table"] [PRESORT]
          [IF …] [WHILE …] [FIRST n | NEXT n]
```
One row per distinct key combination, sorted by the keys. Columns: keys, `OTHER` fields (value from the group's first record), `SUBTOTAL` sums, and `COUNT`. Keys and fields may be expressions with `AS`:
`SUMMARIZE ON MONTH([Txn Date]) AS "Month" SUBTOTAL Amount`

---

## Where results go

| | Output |
|---|---|
| without `TO` | a **session result** (`Results (this session)` in the sidebar). The grid opens it and the next command runs on it. Results are cleared when the app restarts; the newest 30 are kept. Right-click → **Save as table…** to keep one. |
| `TO "name"` | a **permanent table** in the folder. Names must be new and can't start with `_`. |

COUNT and TOTAL return values instead of tables; they appear in the **Output** panel and as variables.

---

## Expressions

| | Syntax |
|---|---|
| Fields | `Amount`, or `[Txn Date]` for names with spaces or that match a keyword (`[Next]`) |
| Text | `"Dar es Salaam"` or `'x'` (double the quote to include it: `"say ""hi"""`) |
| Numbers | `1000`, `12.5` |
| Dates | `` `20240131` ``, date-time `` `20240131 143000` `` |
| Logical | `TRUE`, `FALSE` |
| Compare | `=` `==` `<>` `!=` `<` `<=` `>` `>=` |
| Combine | `AND` `&`, `OR` `\|`, `NOT` `!` |
| Arithmetic | `+ - * /` (text `+` text joins; date `±` number adds days; date `-` date = days; division by 0 gives blank) |
| Variables | `COUNT1`, `TOTAL1`, … from earlier commands: `EXTRACT IF Amount > TOTAL1 / COUNT1` |

Comparisons need matching types: `Amount > "abc"` and `Date > "2024-01-01"` are errors (write dates as `` `20240101` ``).

### Functions

| Function | Result |
|---|---|
| `UPPER(t)`, `LOWER(t)` | text in upper / lower case |
| `ALLTRIM(t)`, `LTRIM(t)`, `TRIM(t)` | spaces removed both sides / leading / trailing |
| `LENGTH(t)` | number of characters |
| `SUBSTR(t, start, length)` | part of the text (start at 1) |
| `FIND("text", field)` | true if field contains the text (case-insensitive) |
| `ISBLANK(x)` | true if empty or only spaces |
| `MATCH(x, v1, v2, …)` | true if x equals any value |
| `BETWEEN(x, low, high)` | true if low ≤ x ≤ high |
| `VALUE(t [, decimals])` | text → number (`"1,250.50"` → 1250.5) |
| `STRING(n)` | number → text |
| `ABS(n)`, `ROUND(n)`, `DEC(n, d)` | absolute / rounded / rounded to d decimals |
| `MAXIMUM(a, b)`, `MINIMUM(a, b)` | larger / smaller value |
| `YEAR(d)`, `MONTH(d)`, `DAY(d)` | parts of a date |

---

## Errors

Every error names the problem and underlines where it is:

```
COUNT IF Amout > 5
         ^^^^^   Unknown field or variable 'Amout'. Did you mean 'Amount'?
```

Every command, successful or not, is written to the folder's command log with the SQL it ran (Output panel → **SQL**).

---

## Differences from ACL (so far)

- Text `=` is an exact comparison (ACL's `SET EXACT OFF` prefix matching isn't emulated).
- `FIND()` takes the field as a second argument.
- Commands not yet available: CLASSIFY, STRATIFY, DUPLICATES, GAPS, JOIN, SAMPLE, STATISTICS, AGE, … (see *Adding a command* in ARCHITECTURE.md).
