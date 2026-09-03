# Launcher Quick Evaluation Design

**Status:** Approved for implementation planning

**Date:** 2026-09-03

## Goal

Extend the main launcher input from four-function arithmetic into a small, strict quick-evaluation surface for:

1. current reference-rate currency conversion;
2. calendar date arithmetic;
3. current time in named cities and IANA time zones; and
4. metric, US customary, and explicitly requested Imperial volume conversion.

The feature keeps the launcher's current single-result, keyboard-first behavior. A successful result is selected automatically and Enter copies it. A confidently recognized but invalid expression shows one non-executable explanatory row. Text that is incomplete or not confidently recognized continues through the existing launcher capability and application-search pipeline.

## Approved Scope

The first release supports English syntax only. Keywords, unit names, aliases, and ISO currency codes are case-insensitive. Fixed UI text remains Chinese; standard identifiers such as currency codes, unit symbols, IANA names, English city names, and English weekday names are not translated.

The four subsystems share classification, numeric parsing, publication, copy execution, and result presentation contracts, but their evaluators remain independent. Currency conversion is the only subsystem that uses the network or persistent cache.

## Relationship To Existing Launcher Contracts

This design extends the classifier described in `2026-08-23-launcher-default-capabilities-design.md`. Its first classification step changes from "valid calculator expression" to "recognized quick-evaluation expression". Empty-input, reserved slash-command, public-plugin, Find, browser-search, and application behavior remain otherwise unchanged.

The arithmetic grammar and results in `src-tauri/src/calculator.rs` remain compatible. In particular, a bare expression such as `2026-08-16` is still arithmetic and evaluates to `2002`; date recognition requires an explicit date unit.

This design also extends the closed `iconKind` set described in `2026-08-17-built-in-result-icons-design.md`. It does not allow applications or public plugins to claim host semantic icons, and it does not change the public plugin API.

All UI work follows `docs/ui-guidelines.md`: the existing 53 px result-row geometry, semantic theme tokens, keyboard selection, Light/Dark parity, stable loading states, and accessible status text remain authoritative.

## Common Query Contract

### Normalization

The quick evaluator receives the outer-trimmed query and rejects input longer than 400 UTF-8 bytes. For keyword and alias matching it collapses runs of ASCII spaces and compares ASCII keywords without case. It does not rewrite the text retained by the launcher control.

The evaluator does not use fuzzy matching. Misspelled keywords, unknown words, currency symbols, and guessed locations never produce a conversion.

### Number Grammar

Currency and unit conversion share this English number grammar:

```text
[optional + or -] integer-or-leading-dot-decimal [optional fractional part]
```

Accepted examples include `12`, `-40`, `.5`, `1.`, and `1,000.5`. Commas are accepted only as correctly grouped thousands separators. A dot is the only decimal separator. `1e6`, `1,5`, `NaN`, infinity, hexadecimal, and Unicode digit forms are not accepted.

The parsed value must be finite. Date quantities use the integer-only subset of the grammar and are additionally constrained by checked calendar arithmetic.

### Classification Result

Every recognizer returns one of three values:

- `NoMatch`: the query is incomplete or is not confidently owned by that recognizer;
- `Invalid`: the query has the recognizer's complete signature but contains an invalid operand, unit, date, city, or provider-supported code; or
- `Ready`: the recognizer produced a valid intent that can be evaluated.

The fixed recognizer order is:

1. currency;
2. date;
3. time zone;
4. unit;
5. arithmetic.

The outer launcher classification remains:

1. try quick evaluation for a non-slash query;
2. handle empty input;
3. handle host-reserved slash commands;
4. handle public-plugin slash discovery or invocation; and
5. handle ordinary text with Find, browser search, plugin suggestions, and applications.

`Ready` and `Invalid` are exclusive results and replace local launcher results. `NoMatch` proceeds to the next recognizer and eventually to ordinary launcher search.

Confidence rules prevent broad interception. For example, `100 USD` is `NoMatch`; `100 USD to XYZ` has the complete currency signature and may become `Invalid`. A unit conversion becomes owned only when the `to` form is complete and at least one side is a known unit. A query ending in `time` is owned by the time-zone recognizer and an unknown location produces an explanatory error rather than a guessed location.

### Internal Boundary

The host owns a `quick_evaluation` module with independent arithmetic, currency, date, time-zone, and unit evaluators. The module exposes an intent/result interface rather than launcher DTOs. It does not read application catalogs, publish registry entries, mutate UI state, or access public-plugin services.

The main search command owns classification order, current-query authorization, conversion of internal results to launcher DTOs, and result-registry actions. Local evaluators are synchronous. Currency evaluation is asynchronous and receives a current-query owner so late network work cannot publish over newer input.

## Currency Conversion

### User Grammar

```text
<number> <three-letter ISO 4217 code> to <three-letter ISO 4217 code>
```

Examples:

```text
100 USD to CNY
1,000.5 eur TO jpy
```

Currency symbols and names such as `$`, `¥`, `元`, and `美元` are out of scope. A syntactically valid three-letter code is checked against provider behavior; unsupported codes produce an `Invalid` result. Converting a code to itself returns the input amount locally with a rate of one and does not use the network.

### Provider Choice And Boundary

The first provider is Frankfurter v2 at the fixed `https://api.frankfurter.dev` origin. Its public API requires no account or API key and exposes latest rates, effective dates, currencies, and provider metadata. Frankfurter documents coverage of 201 currencies from 84 central-bank or institutional sources. This matches the approved "latest published reference rate" requirement; the result is not represented as a tradable market quote.

The host wraps the provider behind an `ExchangeRateProvider` interface. Provider-specific paths, JSON, status mapping, and attribution do not leak into parsing, cache, result publication, or UI code. A later provider replacement must preserve the same internal positive-decimal rate and effective-date contract.

The currency module must not call through the public-plugin network broker or expand that broker's authority. It owns a narrowly scoped HTTPS client with these fixed controls:

- GET only;
- fixed HTTPS origin and hostname;
- exactly two validated three-letter ASCII path segments;
- TLS certificate and hostname validation;
- no redirects;
- a 2-second end-to-end deadline;
- a 64 KiB response-body limit; and
- strict status and JSON validation.

The provider response is accepted only when its base and quote match the request, its date is a valid ISO date, and its rate is a finite positive decimal. Raw provider error bodies and messages are never displayed or logged as launcher text.

### Decimal And Display Semantics

Currency amounts and rates use decimal arithmetic rather than binary floating-point. The converted amount is rounded to at most three digits after the decimal point and trailing zeroes are removed. Negative zero is normalized to `0`.

Example result:

```text
713.245 CNY
100 USD · 1 USD = 7.13245 CNY · Frankfurter · 汇率日期 2026-09-03
```

The rate shown in the subtitle may retain enough provider precision to explain the conversion; the copied amount still follows the three-decimal rule. Enter copies exactly `713.245 CNY`.

### Cache Contract

The application data directory contains a dedicated, versioned, atomically written exchange-rate cache. It stores at most 256 most recently fetched currency pairs. Each entry contains only:

- normalized base code;
- normalized quote code;
- positive decimal rate as a canonical decimal string;
- provider effective date; and
- UTC fetch timestamp.

It does not store the amount, raw query, result text, invocation ID, or application-search history. Entries are evicted by oldest fetch timestamp, not by recording separate user-access metadata.

A directly cached pair or its mathematically inverted reverse pair may satisfy a query. In-flight requests are coalesced by currency pair, including the inverse direction, so editing only the amount does not produce another network call.

Cache age is measured from the successful fetch timestamp:

- up to one hour: fresh and returned immediately;
- over one hour and up to seven days: used only when refresh fails, and labeled `缓存` with its fetch time; and
- over seven days: deleted and never used.

Weekend or holiday reference dates may precede the fetch date; the provider effective date is always shown separately. Only successfully validated provider data is persisted.

The cache follows the repository's atomic current/backup pattern. Loading tries the valid current file, then a valid backup. Unknown schema versions, malformed fields, impossible timestamps, invalid rates, or two unusable files degrade to an empty cache. Because the cache is disposable, cache recovery failure never prevents UiPilot from starting.

### Currency Failure Behavior

- Provider `unsupported currency` behavior becomes a non-executable `Invalid` row.
- Timeout, DNS, TLS, rate-limit, server, size-limit, and malformed-response failures first try the seven-day fallback cache.
- A fallback result clearly says `缓存`, the rate date, and the fetch time.
- If no fallback is available, the row title is `汇率暂不可用` and its subtitle asks the user to retry later.
- A late successful response may update the cache but cannot publish to a stale launcher query owner.

## Date Arithmetic

### User Grammar

```text
(today | YYYY-MM-DD) (+ | -) <non-negative integer>
(day | days | week | weeks | month | months | year | years)
```

Examples:

```text
today + 7 days
today - 2 months
2026-09-03 + 3 weeks
2024-02-29 + 1 year
```

`today` is the Windows local calendar date captured once for that evaluation. If a correct local date cannot be obtained, the evaluator returns a non-executable local-time error; it must not silently substitute UTC, because that can change the date.

An explicit date is valid only in exact `YYYY-MM-DD` form with a year from `0001` through `9999`. The quantity accepts grouped thousands but no fraction or sign; the separate operator determines direction.

### Calendar Semantics

- one week is seven calendar days;
- day and week operations use checked calendar-date arithmetic, not elapsed 24-hour instants;
- month and year operations retain the original day when it exists; and
- when the target month lacks that day, the result clamps to the target month's final day.

Examples:

```text
2026-01-31 + 1 month -> 2026-02-28
2024-02-29 + 1 year  -> 2025-02-28
```

All intermediate multiplication and date construction is checked. A result outside `0001-01-01` through `9999-12-31` is `Invalid`.

The title is the ISO result. The subtitle is the English weekday followed by the normalized expression:

```text
2026-09-10
Thursday · today + 7 days
```

Enter copies exactly `2026-09-10`.

Continuous expressions, business days, holidays, `tomorrow`, `yesterday`, abbreviated date units, localized date input, time-of-day arithmetic, and duration arithmetic are not supported in the first release.

## City And Time-Zone Time

### User Grammar

```text
<city alias | IANA time-zone name> time
```

Examples:

```text
NewYork time
New York time
new-york time
new_york time
America/New_York time
UTC time
```

City alias comparison ignores ASCII case, spaces, hyphens, and underscores. IANA names are looked up case-insensitively in the bundled catalog and restored to their canonical spelling. Ambiguous abbreviations such as `CST`, `EST`, and `IST` are not accepted. `UTC` and `GMT` are explicit fixed-zone aliases and are accepted.

### Initial Alias Catalog

The first release provides explicit aliases for:

- China and East Asia: Beijing, Shanghai, Hong Kong, Taipei, Tokyo, and Seoul;
- Asia and the Middle East: Singapore, Bangkok, Jakarta, Dubai, Mumbai, and New Delhi;
- Europe: London, Paris, Berlin, Rome, and Moscow;
- North America: New York, Chicago, Denver, Los Angeles, San Francisco, Toronto, Vancouver, and Mexico City;
- other common destinations: Sao Paulo, São Paulo, Sydney, Melbourne, and Auckland; and
- UTC and GMT.

Each alias maps to one fixed canonical IANA zone. Unlisted places remain accessible through their full IANA name. Alias additions are data-only changes but must never introduce one normalized alias that maps to multiple zones.

### Clock And Time-Zone Data

Time conversion uses the current system instant and an IANA time-zone database bundled with UiPilot. It does not call an external time API. The database supplies daylight-saving transitions, UTC offsets, and abbreviations; it is refreshed when UiPilot updates its bundled dependency.

The title uses fixed 24-hour `HH:mm` format. The subtitle uses ISO date, canonical English display name, numeric offset, and the current zone abbreviation:

```text
10:35
2026-09-03 · New York · UTC−04:00 (EDT)
```

For a direct IANA query without a city alias, the canonical IANA name is the display name. Enter copies exactly `10:35`.

### Minute Refresh

A successful time response includes a strict UTC `refreshAtUtc` timestamp for the next minute boundary. The frontend schedules one quiet re-query only while the same invocation, view, raw control value, and result owner remain current.

The quiet refresh keeps the current row visible to prevent layout flicker. Beginning the new registry query temporarily disables the retained row's default action until the replacement response arrives, so a visually retained stale result cannot execute against retired authorization. The local evaluator should replace it immediately without a loading-row transition.

Changing input, leaving the launcher, hiding the window, exhausting the query sequence, or destroying the launcher core cancels the timer. A timer firing late after sleep re-evaluates once; it does not replay missed minutes. Every response supplies the next boundary anew.

Unknown aliases, invalid IANA names, ambiguous abbreviations, and unavailable clock/time-zone data produce a non-executable explanatory row and are never guessed.

## Unit Conversion

### User Grammar

```text
<number><optional ASCII space><source unit> to <target unit>
```

Examples:

```text
1000mm to in
1000 millimeters to inches
10 lbs to kg
32°F to Celsius
1 mm^2 to mi2
```

Source and target units must have the same dimension. Conversions within one measurement system are allowed as well as metric-to-US conversions. Signed finite values are accepted for every dimension; temperature adds absolute-zero validation.

### Supported Units

| Dimension | Metric | US customary | Explicit Imperial exception |
| --- | --- | --- | --- |
| Length | `mm`, `cm`, `m`, `km` | `in`, `ft`, `yd`, `mi` | none |
| Mass | `mg`, `g`, `kg`, `tonne`, `metric ton` | `oz`, `lb`, `ton`, `US ton`, `short ton` | none |
| Area | `mm²`, `cm²`, `m²`, `km²`, `ha` | `in²`, `ft²`, `yd²`, `acre`, `mi²` | none |
| Volume | `mL`, `L`, `cm³`, `m³` | `tsp`, `tbsp`, `fl oz`, `cup`, `pt`, `qt`, `gal`, `in³`, `ft³` | prefixed `fl oz`, `pt`, `qt`, `gal` |
| Temperature | `°C`, `K` | `°F` | none |

Every listed unit accepts its conventional abbreviation, English singular name, and English plural name. Meter/metre and liter/litre spellings are both accepted. The degree sign is optional for temperature and is removed only as part of a recognized temperature token.

Area additionally accepts:

- superscript, caret, and digit suffixes such as `mm²`, `mm^2`, and `mm2`;
- `sq in`, `sqin`, and corresponding `sq` forms; and
- full forms such as `square millimeters` and `square inches`.

Volume additionally accepts:

- superscript, caret, and digit suffixes such as `cm³`, `cm^3`, and `cm3`;
- `fl oz`, `floz`, and `fluid ounces`; and
- full forms such as `cubic centimeters`, `cubic inches`, and `cubic feet`.

### US Defaults And Imperial Prefixes

Unprefixed `gal`, `qt`, `pt`, and `fl oz` are US customary units. The explicit `us` prefix selects the same US definitions. The prefixes `imp`, `uk`, and `imperial` select Imperial definitions only for `fl oz`, `pt`, `qt`, and `gal`.

Examples:

```text
100 gal to L      # US gallon
100 us gal to L   # US gallon
100 imp gal to L  # Imperial gallon
```

`cup` is always the US customary cup, exactly `236.5882365 mL`. It is not the 240 mL US legal cup, 250 mL metric cup, or another regional cup. `imp cup` and other undefined prefix/unit combinations are invalid.

Bare `ton` is the US short ton. A metric tonne must be written `tonne` or `metric ton`. Bare `oz` is avoirdupois mass; liquid volume must use `fl oz`, `floz`, or a full fluid-ounce name.

The user-facing explanation is:

> 默认采用美制（US）。若需英制容量（加仑/夸脱/品脱/盎司），请在单位前加 imp，例如 100 imp gal to L。

### Conversion Definitions

Each linear dimension converts through one base unit. Derived factors come from these exact definitions:

- length: `1 in = 0.0254 m`, `1 ft = 12 in`, `1 yd = 3 ft`, `1 mi = 1760 yd`;
- mass: `1 lb = 0.45359237 kg`, `1 oz = 1/16 lb`, `1 US ton = 2000 lb`;
- area: metric powers, squared length definitions, `1 ha = 10,000 m²`, and `1 acre = 43,560 ft²`;
- metric volume: `1 L = 0.001 m³`, `1 mL = 1 cm³`;
- US volume: `1 gal = 3.785411784 L`, with `4 qt = 8 pt = 16 cup = 128 fl oz = 256 tbsp = 768 tsp`, plus cubed exact inch/foot definitions; and
- Imperial volume: `1 imp gal = 4.54609 L`, `1 imp qt = 1/4 imp gal`, `1 imp pt = 1/8 imp gal`, and `1 imp fl oz = 1/160 imp gal`.

Temperature uses affine formulas rather than a shared multiplicative factor:

```text
F = C × 9/5 + 32
K = C + 273.15
```

Inputs below `-273.15 °C`, `-459.67 °F`, or `0 K` are invalid.

### Unit Display

Results use canonical target symbols, at most six significant digits, and no insignificant trailing zeroes. Negative zero becomes `0`.

Normal fixed-decimal output is used when the nonzero absolute value is at least `0.000001` and less than `1,000,000,000`. Values outside that range use normalized scientific notation. Zero always displays as `0`.

Example:

```text
39.3701 in
1000 mm · 长度换算
```

Enter copies exactly `39.3701 in`. A mismatch such as `kg to L` produces the non-executable title `无法转换单位` with a Chinese explanation that the dimensions differ.

## Launcher Result And Wire Contracts

### Internal Result

A successful internal result contains:

- semantic kind;
- title;
- optional subtitle;
- exact copy text; and
- optional next-refresh UTC timestamp.

An invalid result contains semantic kind, Chinese title, and Chinese explanation, but no copy text or refresh request.

### Semantic Icons

The host-only `ResultIconKind` set adds:

```text
currency | date | timeZone | unit
```

Arithmetic retains `calculator`. The frontend uses stable 28 px Lucide-based currency-exchange, calendar, clock, and ruler icons with semantic soft backgrounds. Icons are decorative; the result title remains the accessible name. The additions do not make these transient results `BuiltinFeature` favorites.

Unknown or malformed `iconKind` data is rejected at the field boundary and uses the existing fallback icon behavior without discarding an otherwise valid row.

### Activation And Copy

Successful quick results use `executeResult`, receive a non-empty opaque result ID, and register the existing private `CopyBuiltInText` action. A successful copy occurs before the existing clear-and-hide transaction. Clipboard failure keeps the launcher visible and shows the existing operation error.

The launcher activation protocol adds a host-internal `informational` variant for non-executable error rows. It receives no result action, cannot call `execute_result`, and carries `hasDefaultAction = false`. This preserves the existing closed rule that `executeResult` always refers to a registry-authorized action instead of publishing a fake executable error row.

No quick result exposes favorite metadata or a context menu.

### Refresh Metadata

`SearchResponse` adds optional `refreshAtUtc`. Only the time-zone evaluator sets it. The frontend accepts only a strict UTC RFC 3339 timestamp; an unknown, malformed, or stale value is ignored while retaining valid rows. Refresh ownership is checked independently of presentation fields, so an icon cannot request background work.

## UI States

- Success: one selected semantic result row; Enter and click copy.
- Confident invalid expression: one selected informational row; Enter and click do nothing.
- Cold currency request: retain the launcher's stable result-area loading treatment for at most two seconds.
- Fresh cache: publish immediately with provider and effective-date metadata.
- Stale fallback cache: publish immediately after refresh failure and visibly label it `缓存`.
- Currency unavailable: one informational row; do not mix in Find, browser, plugin, or application rows.
- Incomplete/unowned text: use the existing ordinary launcher results without a quick-evaluation error.

Static UI words and error explanations are Chinese. Currency codes, canonical unit symbols, English weekdays, English city names, and IANA identifiers retain their standard forms.

## Data Flow

1. The frontend sends the raw query with the current invocation and increasing query sequence.
2. The backend outer-trims for classification while preserving the raw owner.
3. A non-slash query passes through the fixed recognizer order.
4. `NoMatch` continues through the existing launcher classifier.
5. A local `Ready` or any `Invalid` result is mapped immediately to one owned launcher response.
6. A currency `Ready` intent checks fresh cache, then coalesced network work, then stale fallback.
7. The result registry publishes only for the latest invocation and sequence.
8. The frontend independently rechecks invocation, sequence, view epoch, control key, and raw control value before accepting the response.
9. A successful activation resolves its private copy action, writes the clipboard, then clears and hides.
10. A valid `refreshAtUtc` schedules the guarded quiet-refresh flow described above.

## Failure And Safety Rules

- No parse failure panics or invokes an external side effect.
- All numeric, date, multiplication, inversion, and range operations are checked.
- Provider responses are untrusted and strictly bounded before deserialization and publication.
- User text cannot select a hostname, port, scheme, request header, arbitrary path, or redirect target.
- A stale query cannot publish, copy, hide the launcher, or schedule a new refresh.
- Cache corruption cannot prevent application startup or local quick evaluation.
- Error rows never carry executable registry actions.
- Raw queries and provider error bodies are not persisted or logged by this feature.
- Local clock failure is explicit; UTC is not silently substituted for `today`.
- Unsupported currency, city, IANA zone, unit, prefix, or cross-dimension pair is never guessed.

## Testing Strategy

Automated tests use injected clocks, local-date sources, time-zone catalogs, exchange-rate providers, transports, and cache paths. Default tests never access the live provider.

### Parser And Routing Tests

- table tests for every `NoMatch`, `Invalid`, and `Ready` boundary;
- English case folding, outer trimming, internal spaces, comma grouping, signs, leading-dot decimals, and rejected scientific/localized numbers;
- fixed recognizer ordering and exclusive publication;
- partial expressions falling through to ordinary search;
- calculator regression cases, including bare `2026-08-16`;
- empty input, `/find`, `/web-search`, public-plugin discovery/invocation, Find, browser search, and application-order regressions; and
- proof that recognized local and currency-cache results do not read the application catalog.

### Currency Tests

- case-insensitive ISO codes, identical codes, three-decimal rounding, and trailing-zero removal;
- fresh, stale, expired, reverse-pair, corrupt-current, valid-backup, unknown-schema, and 256-entry eviction behavior;
- successful validation and rejection of mismatched pairs, invalid dates, zero/negative rates, oversized bodies, redirects, malformed JSON, unsupported codes, and non-success status classes;
- timeout and provider failure with and without a seven-day fallback;
- same-pair and inverse-pair in-flight request coalescing;
- old network work unable to publish over a newer query; and
- persisted cache containing no amount or raw query.

### Date Tests

- day/week arithmetic across month and year boundaries;
- January month-end clamping and leap-day year clamping;
- strict ISO validation, grouped integer quantities, fractional rejection, checked multiplication, and date-range overflow;
- injected Windows local dates around local midnight; and
- ISO title, English weekday subtitle, and exact copy text.

### Time-Zone Tests

- every declared city alias and its separator/case variants;
- canonical IANA lookup and rejection of ambiguous abbreviations;
- New York before, during, and after daylight-saving transitions;
- fixed `HH:mm`, ISO date, UTC offset, abbreviation, and copy output;
- next-minute `refreshAtUtc` calculation;
- quiet refresh canceled by editing, hiding, navigation, sequence exhaustion, and destruction;
- late wake causing one refresh rather than replay; and
- malformed refresh metadata ignored without discarding valid rows.

### Unit Tests

- every canonical unit and every accepted singular, plural, exponent, square/cubic, spelling, and fluid-ounce alias;
- exact anchor definitions and representative forward/reverse conversions;
- unprefixed US volume, explicit `us`, explicit `imp/uk/imperial`, US customary cup, US short ton, metric tonne, mass ounce, and fluid ounce;
- rejection of undefined prefix/unit combinations and cross-dimension pairs;
- all Celsius/Fahrenheit/Kelvin directions and absolute-zero boundaries;
- six-significant-digit formatting, scientific-notation thresholds, trailing-zero removal, and negative-zero normalization; and
- canonical target symbols and exact copy text.

### Protocol And UI Tests

- exact new icon values plus existing unknown-field fallback;
- `informational` rows retained but never executed;
- successful copy occurring before clear-and-hide and clipboard failure keeping the launcher visible;
- default selection, arrow navigation, mouse activation, Enter, and status-announcement behavior;
- stable loading and quiet-refresh layout; and
- Light, Dark, selected, focus, forced-colors, long-subtitle, and error-state rendering checks.

A real Frankfurter call is a manual smoke check only. It verifies current service reachability, supported sample codes, displayed effective date, and attribution, but it is not a release-gating automated test.

## Acceptance Criteria

1. `100 USD to CNY` returns one current reference-rate result, and Enter copies an amount with `CNY`.
2. A fresh rate is reused for one hour; provider failure may use a clearly labeled cache fetched no more than seven days earlier.
3. No API key, currency settings UI, or raw-query persistence is introduced.
4. `today + 7 days` and `YYYY-MM-DD` expressions for days, weeks, months, and years obey the approved calendar and month-end rules.
5. `NewYork time` and `America/New_York time` show the correct current offset and daylight-saving abbreviation and refresh each minute while still current.
6. `1000 mm to in` returns `39.3701 in` and Enter copies that exact text.
7. Every approved metric and US unit, plus explicitly prefixed Imperial volume units, follows the exact definitions and ambiguity rules in this document.
8. Invalid complete expressions show one non-executable explanation; incomplete or unrelated text preserves ordinary launcher results.
9. Existing arithmetic, slash commands, public plugins, Find, browser search, applications, result authorization, clipboard ordering, and launcher hide behavior continue to pass regression tests.
10. New result icons and states satisfy the existing UI guideline in Light and Dark themes without changing row geometry.

## Non-Goals

- Chinese or locale-dependent input syntax.
- Currency symbols, currency names, historical exchange rates, trading quotes, a paid provider, or user-supplied API keys.
- A secondary live currency provider or provider-selection setting.
- Arbitrary online city geocoding or a time API.
- Business-day, holiday, natural-language, time-of-day, or duration arithmetic.
- Scientific-notation input or comma decimal separators.
- Units outside the approved table, user-defined units, US legal/metric/regional cups, or general Imperial length and mass.
- Favoriting individual conversion results.
- Changes to the public plugin manifest, runtime API, permissions, network broker, or SDK contracts.

## References

- [Frankfurter v2 API](https://frankfurter.dev/)
- [ECB Data API](https://data.ecb.europa.eu/help/getting-data-web-services-sdmx-0)
- [ExchangeRate-API open endpoint terms and update frequency](https://www.exchangerate-api.com/docs/free)
- `docs/ui-guidelines.md`
- `docs/superpowers/specs/2026-08-23-launcher-default-capabilities-design.md`
- `docs/superpowers/specs/2026-08-17-built-in-result-icons-design.md`
