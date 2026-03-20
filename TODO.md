# Glass Compiler — TODO

## Milestone 1–9: Completed (summary)

- [x] M1: Lexer, parser, AST, basic codegen, 114 tests
- [x] M2: Types → SoA, pattern matching, record update, tuples, lists
- [x] M3: Closures (alloc + capture), pipe operator
- [x] M4 (partial): Elm runtime preamble, init/update, one-shot effects
- [x] M5: Move checker, auto-cleanup, local fn checker
- [x] M6: common.j parser (1409 natives), auto-bindings, miette errors
- [x] M7: HM type checker, exhaustiveness, advanced patterns, monomorphization, typed AST, function mono, CLI (clap)
- [x] M8: stdlib (Option, Result, List, Int/Float/String, Dict, Set)
- [x] M9 (partial): Module system, DCE, constant folding, 302 tests

---

## Milestone 10: Runtime

- [x] **10.1 Closure dispatch** — all lambdas take glass_clos_id, dispatch by signature group, runtime uses glass_dispatch_void.
- [x] **10.2 Рекурсия + list pattern matching** — factorial, `[h | t]`, head/tail extraction.
- [x] **10.3 @external end-to-end** — proper JASS handle types, native names resolved, pjass validates.
- [x] **10.4 Pure Effects** — `sdk/effect.glass` defines `Effect(M)` ADT (After, DisplayText). `update` returns `#(Model, List(Effect(Msg)))`. Runtime walks effect list and executes. Timer callback self-contained (avoids JASS forward ref cycle).
- [ ] **10.5 Subscriptions + reconciliation** — deferred (not needed for MVP).

## Milestone 11: Codegen correctness (exposed by tower_defense.glass)

**CRITICAL (blocks tower_defense.glass):**

- [x] **11.1 Closure CALL dispatch** — closure parameter calls now generate `glass_dispatch_N(f, args)` instead of `glass_f(args)`. All stdlib higher-order functions (list.map, list.filter, option.map, list.fold) produce valid JASS.

- [x] **11.2 Enum tag access** — `glass_tag(x)` → `glass_{TypeName}_tag[x]`. Case subject type looked up from type_map. Bool dispatch subjects wrapped with `glass_i2b()` when coming from dispatch calls.

- [x] **11.3 Field access for variant types** — `glass_get_Playing_wave(x)` → `glass_get_Phase_Playing_wave(x)`. Type name missing from getter. Fixed for single-variant types via type_map; broken for vars bound in pattern arms of multi-variant case.

- [ ] **11.4 Positional field access** — `glass_field_0(x)` generated for `Constructor(val)` patterns. Should be `glass_get_{Type}_{Variant}_{field}[x]` like named fields.

- [x] **11.5 @external resolution for qualified module calls** — `int.to_string(x)` → `I2S(x)`, `float.to_string(y)` → `R2S(y)`. Qualified external names resolve correctly.

- [x] **11.6 Module name collision** — `import int` + `import float` no longer breaks. Fix: module resolver deduplicates by qualified name (module.fn), inferencer maps each definition to its source module, colliding unqualified names are not bound (only qualified access works). Remaining issue: DCE keeps both versions of colliding imported pub functions → duplicate JASS function definitions when both modules imported.

- [ ] **11.6b Duplicate imported functions in codegen** — when two modules export same-named pub functions (e.g. `int.min` and `float.min`), both end up in JASS output as `glass_min` causing a redefinition error. Fix: either qualify JASS names (`glass_int_min`) or improve DCE to only keep imported functions reachable from user code.

- [x] **11.7 Lambda `_` parameter** — `fn(_: a)` generates `glass_unused_N`.

- [x] **11.8 Temp vars for imported function bodies** — fixed: temp_counter reset per function, body buffered, temps declared after generation. Dedup of locals via HashSet.

- [x] **11.9 SoA primitive field types** — `After { duration: Float }` now generates `real array`. Float/String/Bool/Unit/Sfx fields all get correct JASS array types. Typed pattern locals from ConstructorNamed patterns.

**Lower priority:**

- [ ] **11.10 `todo()` expression** — compile to runtime crash.
- [ ] **11.11 `extend` blocks codegen** — not implemented.

## Milestone 11b: Codegen correctness (fixes applied)

- [x] **11b.1 Temp variable per-function reset** — `fresh_temp()` global counter reset to 0 per function. Body buffered, locals declared after.
- [x] **11b.2 Typed temp variables** — case expression result temps get correct JASS type (boolean, real, etc.) from type_map instead of always integer.
- [x] **11b.3 Boolean dispatch conversion** — `glass_i2b(integer) → boolean` helper. Case subjects from dispatch wrapped automatically.
- [x] **11b.4 Duplicate local dedup** — case arms binding same variable name no longer produce duplicate JASS local declarations (HashSet dedup).
- [x] **11b.5 Typed ConstructorNamed pattern locals** — field JASS types looked up from TypeRegistry, so Unit/Sfx/Float fields declare correct local type.
- [x] **11b.6 `Sfx` handle type** — Glass type mapping to JASS `effect` handle (avoids collision with `Effect(M)` ADT).
- [x] **11b.7 `clone(handle)` allowed** — linearity checker now permits clone for handle types (WC3 runtime is ref-counted). New `Borrowed` state.
- [x] **11b.8 Constructor consumes handles** — handle passed as ADT constructor argument marked as Moved.
- [x] **11b.9 Case arm handle state merge** — after case, handle states merged across arms (Moved > Borrowed > Alive).
- [x] **11b.10 Exhaustiveness skips imports** — exhaustiveness checker skips imported definitions (wrong spans from merged modules).

## Milestone 11c: SDK (new modules)

- [x] **11c.1 `sdk/wc3/math.glass`** (was `jass/`) — sin, cos, atan2, sqrt, random_int, random_real, deg2rad, rad2deg.
- [x] **11c.2 `sdk/wc3/unit.glass`** (was `jass/`) — get_x, get_y, set_x, set_y, set_pos, get_facing, create, remove, handle_id.
- [x] **11c.3 `sdk/wc3/sfx.glass`** (was `jass/`) — at_point, on_unit, destroy.

## Milestone 12: Юзабилити

- [ ] **12.1 Multiline expressions** — verify pipe chains, case arms parse across line breaks.
- [ ] **12.2 Better error messages** — "did you mean?", arg count mismatches, unknown fields.
- [ ] **12.3 Watch mode** — `glass watch file.glass`.
- [ ] **12.4 LSP / editor integration** — tree-sitter grammar or minimal language server.

## Milestone 13: Демо

- [x] **13.1 Spell examples** — Greater Bash (PRD + knockback + dust trail) and Axes of Rexxar (bouncing damage + cooldowns). 3 examples total, all compile to valid JASS (pjass-validated), 306 tests.
- [ ] **13.2 Tower Defense** — full game on Glass.

---

## Milestone 14: SDK rename + WC3 native bindings

### 14.1 Rename `sdk/jass/` → `sdk/wc3/`
- [x] **14.1.1** Rename directory `sdk/jass/` → `sdk/wc3/`
- [x] **14.1.2** Update all imports in examples (`import jass/...` → `import wc3/...`)
- [x] **14.1.3** Update TODO.md, DOCS.md references
- [x] **14.1.4** Tests pass, fmt, clippy clean

### 14.2 WC3 SDK: Player & core
- [x] **14.2.1** `sdk/wc3/player.glass` — `Player(id)`, `GetLocalPlayer`, `GetPlayerId`, `GetTriggerPlayer`, `GetOwningPlayer`, `GetPlayerName`, `GetPlayerGold`, `SetPlayerGold`, `AdjustPlayerGold`
- [x] **14.2.2** Tests pass

### 14.3 WC3 SDK: Unit combat
- [x] **14.3.1** Expand `sdk/wc3/unit.glass` — `GetUnitState`, `SetUnitState`, `UnitDamageTarget`, `KillUnit`, `IsUnitAliveBJ`, `SetUnitAnimation`, `PauseUnit`, `ShowUnit`, `SetUnitInvulnerable`, `GetUnitTypeId`, `SetUnitOwner`, `SetHeroLevel`, `GetHeroLevel`, `GetHeroXP`, `AddHeroXP`, `SetHeroStr`/`Agi`/`Int`, `GetHeroStr`/`Agi`/`Int`, `UnitAddType`, `UnitRemoveType`
- [x] **14.3.2** Tests pass

### 14.4 WC3 SDK: Ability & buff
- [x] **14.4.1** `sdk/wc3/ability.glass` — `UnitAddAbility`, `UnitRemoveAbility`, `SetUnitAbilityLevel`, `GetUnitAbilityLevel`, `IncUnitAbilityLevel`, `UnitMakeAbilityPermanent`
- [x] **14.4.2** Tests pass

### 14.5 WC3 SDK: Item
- [x] **14.5.1** `sdk/wc3/item.glass` — `CreateItem`, `RemoveItem`, `UnitAddItem`, `GetItemTypeId`, `GetItemName`, `GetItemCharges`, `SetItemCharges`, `GetManipulatedItem`, `GetItemOfTypeFromUnitBJ`, `UnitHasItemOfTypeBJ`
- [x] **14.5.2** Tests pass

### 14.6 WC3 SDK: Timer
- [x] **14.6.1** `sdk/wc3/timer.glass` — `CreateTimer`, `DestroyTimer`, `TimerStart`, `PauseTimer`, `ResumeTimer`, `GetExpiredTimer`, `TimerGetRemaining`, `TimerGetElapsed`
- [x] **14.6.2** Tests pass

### 14.7 WC3 SDK: Group
- [x] **14.7.1** `sdk/wc3/group.glass` — `CreateGroup`, `DestroyGroup`, `GroupAddUnit`, `GroupRemoveUnit`, `GroupEnumUnitsInRange`, `FirstOfGroup`, `IsUnitInGroup`, `GroupClear`
- [x] **14.7.2** Tests pass

### 14.8 WC3 SDK: UI
- [x] **14.8.1** `sdk/wc3/ui.glass` — `DisplayTimedTextToPlayer`, `ClearTextMessages`, `CreateTextTag`, `SetTextTagText`, `SetTextTagPos`, `SetTextTagColor`, `SetTextTagVelocity`, `SetTextTagLifespan`, `SetTextTagPermanent`, `DestroyTextTag`
- [x] **14.8.2** Tests pass

### 14.9 WC3 SDK: Camera
- [x] **14.9.1** `sdk/wc3/camera.glass` — `SetCameraPosition`, `PanCameraTo`, `PanCameraToTimed`, `SetCameraField`, `ResetToGameCamera`, `GetCameraTargetPositionX/Y`
- [x] **14.9.2** Tests pass

### 14.10 WC3 SDK: Sound
- [x] **14.10.1** `sdk/wc3/sound.glass` — `PlaySoundBJ`, `StopSound`, `SetSoundVolume`, `CreateSound`, `StartSound`, `KillSoundWhenDone`
- [x] **14.10.2** Tests pass

### 14.11 WC3 SDK: Region & Rect
- [x] **14.11.1** `sdk/wc3/rect.glass` — `Rect`, `RemoveRect`, `GetRectCenterX/Y`, `GetRectMinX/Y`, `GetRectMaxX/Y`, `RectContainsUnit`
- [x] **14.11.2** Tests pass

### 14.12 WC3 SDK: Destructable
- [x] **14.12.1** `sdk/wc3/destructable.glass` — `CreateDestructable`, `RemoveDestructable`, `KillDestructable`, `GetDestructableLife`, `SetDestructableLife`
- [x] **14.12.2** Tests pass

### 14.13 WC3 SDK: Multiboard
- [x] **14.13.1** `sdk/wc3/multiboard.glass` — `CreateMultiboard`, `DestroyMultiboard`, `MultiboardDisplay`, `MultiboardSetTitleText`, `MultiboardSetColumnCount`, `MultiboardSetRowCount`, `MultiboardGetItem`, `MultiboardSetItemValue`, `MultiboardSetItemWidth`, `MultiboardReleaseItem`
- [x] **14.13.2** Tests pass

---

## Milestone 15: Subscriptions & Effects expansion

### 15.1 New subscriptions
- [x] **15.1.1** `OnSpellCast { handler: fn(Int, Int) -> M }` — caster_id, spell_id
- [x] **15.1.2** `OnSpellChannel { handler: fn(Int, Int) -> M }` — caster_id, spell_id
- [x] **15.1.3** `OnDamage { handler: fn(Int, Int, Int) -> M }` — source_id, target_id, amount
- [x] **15.1.4** `OnItemUse { handler: fn(Int, Int) -> M }` — unit_id, item_id
- [x] **15.1.5** `OnItemDrop { handler: fn(Int, Int) -> M }` — unit_id, item_id
- [x] **15.1.6** `OnUnitEntersRegion { handler: fn(Int) -> M }` — unit_id
- [x] **15.1.7** `OnChat { handler: fn(Int, String) -> M }` — player_id, message
- [x] **15.1.8** `OnPlayerLeave { handler: fn(Int) -> M }` — player_id
- [x] **15.1.9** `OnHeroLevelUp { handler: fn(Int) -> M }` — hero_id
- [x] **15.1.10** `OnConstructionFinish { handler: fn(Int) -> M }` — building_id
- [x] **15.1.11** Tests pass, fmt, clippy clean

### 15.2 New effects
- [x] **15.2.1** `DamageUnit { source_id: Int, target_id: Int, amount: Float, attack_type: Int, damage_type: Int }`
- [x] **15.2.2** `CreateUnit { owner: Int, type_id: Int, x: Float, y: Float, facing: Float }`
- [x] **15.2.3** `RemoveUnit { unit_id: Int }`
- [x] **15.2.4** `MoveUnit { unit_id: Int, x: Float, y: Float }`
- [x] **15.2.5** `PlayAnimation { unit_id: Int, anim: String }`
- [x] **15.2.6** `AddAbility { unit_id: Int, ability_id: Int }`
- [x] **15.2.7** `AddSfx { model: String, x: Float, y: Float }`
- [x] **15.2.8** `SetUnitHp { unit_id: Int, hp: Float }`
- [x] **15.2.9** `SetUnitMana { unit_id: Int, mana: Float }`
- [x] **15.2.10** `PanCamera { player_id: Int, x: Float, y: Float }`
- [x] **15.2.11** `ShowFloatingText { text: String, x: Float, y: Float, size: Float }`
- [x] **15.2.12** `PlaySound { path: String }`
- [x] **15.2.13** `Batch { effects: List(Effect(M)) }`
- [x] **15.2.14** Update runtime to handle new effects (both JASS + Lua)
- [x] **15.2.15** Tests pass, fmt, clippy clean

---

## Milestone 16: Standard library expansion

### 16.1 List
- [x] **16.1.1** `take(n)`, `drop(n)`, `enumerate`, `concat` (flatten), `intersperse`, `sum`, `product`, `partition`
- [x] **16.1.2** Tests pass

### 16.2 Int
- [x] **16.2.1** `pow`, `sign`, `is_even`, `is_odd`
- [x] **16.2.2** Tests pass

### 16.3 Float
- [x] **16.3.1** `floor`, `ceil`, `round`, `pi`, `lerp`
- [x] **16.3.2** Tests pass

### 16.4 String
- [x] **16.4.1** `contains`, `starts_with`, `repeat`, `char_at`, `index_of`
- [x] **16.4.2** Tests pass

### 16.5 Dict
- [x] **16.5.1** `remove`
- [x] **16.5.2** Tests pass

### 16.6 New module: `math.glass`
- [x] **16.6.1** `distance(x1,y1,x2,y2)`, `angle_between(x1,y1,x2,y2)`, `move_point(x,y,angle,dist)`, `lerp`, `normalize_angle`, `clamp_angle`
- [x] **16.6.2** Tests pass

### 16.7 New module: `color.glass`
- [x] **16.7.1** `rgb(r,g,b) -> String` — WC3 color code `"|cFFrrggbb"`, `red()`, `green()`, `blue()`, `yellow()`, `gold()`, `white()`, `gray()`
- [x] **16.7.2** Tests pass

---

## Milestone 17: Game example — полноценная карта

### 17.1 Sniper PRD с реальным random
- [x] **17.1.1** `roll_headshot` использует `wc3/math.random_int` вместо fake rng
- [x] **17.1.2** `main.glass` Msg::UnitAttacked убирает rng поле
- [x] **17.1.3** Tests pass

### 17.2 Map setup
- [x] **17.2.1** `game/map/setup.glass` — создание героев для игроков при старте (`wc3/unit.create`), rawcodes (`'Hpal'`, `'Udre'`, `'Edem'`)
- [x] **17.2.2** `game/map/regions.glass` — spawn points, shop areas, waypoints как координаты
- [x] **17.2.3** `init()` вызывает setup, создаёт юнитов, заполняет Model
- [x] **17.2.4** Tests pass

### 17.3 Боевая система
- [x] **17.3.1** `game/systems/damage.glass` — модификаторы урона, DamageUnit effect
- [x] **17.3.2** handle_attack реально наносит урон через DamageUnit effect
- [x] **17.3.3** handle_kill → respawn timer, gold reward
- [x] **17.3.4** Tests pass

### 17.4 Новый герой: Paladin
- [x] **17.4.1** `game/heroes/paladin.glass` — Holy Light (heal), Divine Shield (invul), Resurrection
- [x] **17.4.2** Интеграция в main.glass (Msg, subscriptions, update)
- [x] **17.4.3** Tests pass

### 17.5 Предметы и магазин
- [x] **17.5.1** `game/items/shop.glass` — покупка/продажа, gold tracking, item list
- [x] **17.5.2** Интеграция: OnItemPickup, BuyItem msg
- [x] **17.5.3** Tests pass

### 17.6 Волны крипов (реальные)
- [x] **17.6.1** `game/map/creeps.glass` расширить — CreateUnit effect, rawcodes, wave scaling
- [x] **17.6.2** Patrol waypoints через MoveUnit
- [x] **17.6.3** Tests pass

### 17.7 Система баффов
- [x] **17.7.1** `game/systems/buffs.glass` — Haste, Slow, Regen, DoubleDamage с таймерами
- [x] **17.7.2** Интеграция в update: apply/remove/tick
- [x] **17.7.3** Tests pass

### 17.8 UI и scoreboard
- [x] **17.8.1** `game/ui/scoreboard.glass` — multiboard с kills/deaths/gold
- [x] **17.8.2** `game/ui/messages.glass` — floating damage, kill feed, цветные сообщения
- [x] **17.8.3** Tests pass

### 17.9 Camera & Sound
- [x] **17.9.1** Camera lock на героя, pan при событиях
- [x] **17.9.2** Звуки при убийстве, касте
- [x] **17.9.3** Tests pass

### 17.10 Финал
- [x] **17.10.1** Полная компиляция game/main.glass в JASS + Lua, pjass/luac validation
- [x] **17.10.2** Всё собрано, тесты зелёные, clippy clean

---

## Milestone 18: Optimizations

### Phase 1 — Quick wins (no IR needed)

- [x] **18.1 Name mangling** — frequency-based AST analysis, shortest names (a,b,...,z,aa,...) for most-used identifiers
  - `--no-mangle` opt-out flag, reserved set (keywords + all user vars), no global/local shadowing
  - JASS + Lua, 3 conflict-prevention tests, 386 tests green

- [x] **18.1b Strip** — remove blank lines + comment lines from output (`--no-strip` opt-out)

- [ ] **18.1c Mangle residuals** *(low priority)* — some `glass_*` names survive mangling:
  - `glass_tmp_N` — codegen temp locals (not in AST frequency table)
  - `glass_closure`, `glass_tag`, `glass_cid`, `glass_pN` — dispatch function params (hardcoded in codegen)
  - `glass_Tuple2_integer_integer_*` — monomorphized tuple SoA names (generated during mono, not in AST)
  - `glass_msg_*`, `glass_timer_ht`, `glass_handle_ht`, `glass_closN_*` — runtime/closure infra
  - Fix: either predict these names during AST analysis, or add a second mangling pass on codegen-internal names

- [x] **18.2 Tail Call Optimization** — tail-recursive functions → loops
  - AST pass: detect all self-calls in tail position, transform to `TcoLoop`/`TcoContinue`
  - JASS: `loop`/`endloop` with temp-based param reassignment (safe evaluation order)
  - Lua: native TCO via `return f(...)` in tail position
  - `--no-tco` opt-out, topo sort handles TCO'd function dependencies, 391 tests green

- [ ] **18.3 Beta reduction** — inline immediately-applied lambdas
  - `(\x -> x + 1)(5)` → `5 + 1`
  - Simple AST pass

### Phase 2 — Lambda lifting + Inlining

- [ ] **18.4 Lambda lifting** — closures → top-level functions with explicit capture args
  - No-capture lambdas → plain functions (zero overhead)
  - Captured vars become extra parameters
  - Eliminates closure allocation + dispatch when combined with inlining

- [ ] **18.5 Inlining** — inline small / single-use functions at call sites
  - **Порядок: lambda lifting → inlining → DCE** (lifting делает closures видимыми как обычные функции, inlining подставляет HOF на call site, аргумент-функция становится известна статически → dispatch заменяется прямым вызовом, DCE убирает неиспользуемые lifted функции)
  - **Всегда инлайнить (без бюджета):** single-use функции (любой размер тела), тривиальные обёртки (тело = 1 вызов/переменная), конструкторы (`Some(x)`, `Ok(x)`)
  - **По размеру тела (не по числу вызовов):** маленькая функция дешевле инлайнить 50 раз, чем 50 раз дёргать call/dispatch
  - **Никогда:** рекурсивные функции, `@external`
  - **Модель стоимости:** Var/Int/Float/Bool/String=0, BinOp/UnaryOp=1, Constructor/Let=1, Call/Case=2. Порог ~10-15
  - **Результат с lifting:** HOF (List.map, List.filter и т.д.) инлайнятся на call site → аргумент-функция известна → прямой вызов вместо `glass_dispatch_N` → closure allocation и dispatch исчезают. Оставшиеся closures — только реально динамические (функция выбирается в runtime)

- [ ] **18.6 Closure cleanup** — defunctionalize remaining closures
  - After lifting + inlining, only truly dynamic dispatch remains
  - Known call sites → direct calls instead of `glass_dispatch_N`

- [ ] **18.7 Constant propagation** — propagate let-bound constants
  - `let x = 5 in ... x ...` → substitute 5
  - Extend from top-level `const` to `let` bindings

### Phase 3 — Advanced (introduce ANF IR here)

- [ ] **18.8 List fusion / deforestation** — eliminate intermediate lists
  - `list |> List.map f |> List.filter g` → single pass
  - Critical for JASS: each intermediate list = SoA allocations

- [ ] **18.9 Case simplification** — pattern match → decision trees
  - Eliminate redundant tag checks in nested matches

- [ ] **18.10 Common subexpression elimination**
  - `f(x) + f(x)` → `let tmp = f(x) in tmp + tmp`
  - Glass functions are pure (except externals) → safe
