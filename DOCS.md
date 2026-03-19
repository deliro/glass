# Glass: Функциональный язык → JASS Compiler

## Context

Компилятор функционального языка в JASS (Warcraft 3) с Elm-архитектурой. Rust проект: `/Users/tochkamac/projects/own/glass`. Свой парсер, свой синтаксис.

---

## Ключевые решения

### 1. Ownership по умолчанию (Rust-like)

Все типы **линейные по умолчанию** (move semantics). Явные маркеры:
- Примитивы (`Int`, `Float`, `Bool`, `String`) — автоматически `Copy`
- Handle типы (`Unit`, `Timer`, `Group` и т.д.) — линейные, move-only
- Пользовательские типы — линейные, если не помечены

```rust
// Примитивы — Copy
// Handle типы (Unit, Timer, Group) — линейные, move-only (но clone разрешён)

let a = Model { wave: 1 }
let b = clone(a)               // OK — clone создаёт алиас (WC3 ref-counted)
let c = a                      // a moved в c, больше нельзя использовать a
```

Компилятор вставляет cleanup (DestroyTimer, RemoveUnit, set x = null) автоматически когда линейное значение выходит из scope.

### 2. `local fn` — desync-safe функции

**Проблема:** В мультиплеере WC3 все игроки выполняют один и тот же JASS код одновременно. `GetLocalPlayer()` возвращает разного игрока на каждой машине. Если внутри блока с `GetLocalPlayer()` создать handle (юнит, таймер, группу), handle ID будут разными у разных игроков → **desync** (рассинхрон, краш игры).

Опасные операции внутри local-контекста:
- Создание/уничтожение handle (`CreateUnit`, `CreateTimer`, `DestroyGroup`)
- `ForGroup`, `ForForce` (внутри создают handle)
- `TriggerSleepAction` (desync по таймингу)
- Изменение game state (damage, move, create)

Безопасные операции:
- Камера (`SetCameraPosition`, `PanCameraTo`)
- Звук (`PlaySound`, `SetSoundVolume`)
- Визуальные эффекты (`AddSpecialEffect` — спорно, но обычно ОК)
- Чтение state (`GetUnitX`, `GetPlayerName`)
- UI (`DisplayTimedTextToPlayer` для конкретного игрока)

**Решение: `local fn`** — функции, которые могут содержать GetLocalPlayer-зависимый код. Компилятор трекает все вызовы внутри и запрещает опасные:

```rust
local fn update_camera(model: Model) {
  let p = get_local_player()
  if p == Player(0) {
    set_camera_position(model.hero_x, model.hero_y)  // OK: камера
  }
  // create_unit(...)  // ОШИБКА КОМПИЛЯЦИИ: handle creation in local fn
}

fn update(model: Model, msg: Msg) -> #(Model, List(Effect(Msg))) {
  // get_local_player()  // ОШИБКА: не в local fn
  // ...
}
```

`local fn` может вызывать только другие `local fn` и чистые функции. Обычная `fn` не может вызывать `local fn`. Это **заразная** аннотация (как `async` в Rust, но для desync safety). Вызов `local fn` происходит через эффекты или subscriptions.

### 3. Subscriptions как часть Elm-архитектуры

Подписки — **третья ножка** Elm-архитектуры наравне с `init` и `update`. Решают проблему дублирования `effect.every`:

```
init       → начальная модель + one-shot эффекты
update     → обработка сообщения → новая модель + one-shot эффекты
subscriptions → текущие подписки (зависят от модели)
```

**Правило:** `update` возвращает только **one-shot** эффекты (создать юнита, показать текст, запустить одноразовый таймер). Все **ongoing** вещи (периодические таймеры, trigger'ы на события) живут в `subscriptions`:

```rust
fn subscriptions(model: Model) -> List(Sub(Msg)) {
  case model.phase {
    Playing -> [
      sub.every(1.0, fn() { Tick }),
      sub.on_unit_death(fn(dying, killer) { UnitKilled(killer, dying) }),
    ]
    Lobby -> [
      sub.on_chat("-start", fn(player) { StartGame(player) }),
    ]
    Victory -> []
  }
}
```

**Runtime reconciliation:** после каждого `update` runtime вызывает `subscriptions(new_model)` и сравнивает с предыдущим набором подписок:
- Новая подписка → `CreateTrigger`/`CreateTimer`
- Удалённая подписка → `DestroyTrigger`/`DestroyTimer`
- Без изменений → ничего

**Identity подписок:** для reconciliation нужно уметь сравнивать подписки. Варианты:
- По позиции в списке (как React keys) — просто, но хрупко
- Явный `key`: `sub.every(1.0, fn() { Tick }) |> sub.key("game_tick")` — надёжно

### 4. SoA + freelist менеджмент

Массивы вместо hashtable для типов. Freelist для переиспользования ID.

**Почему НЕ swap_remove:** swap_remove меняет ID элемента при удалении (свапает с последним). Это ломает все ссылки на перемещённый элемент. В Elm-архитектуре ID — это "указатели", они хранятся в модели и других структурах. Обновить все ссылки слишком дорого.

**Решение: обычный freelist** (стек свободных ID). Дырки в массивах — ОК, потому что:
- Мы не итерируем по всем экземплярам типа (обращаемся по ID)
- В Elm-архитектуре модель перестраивается каждый update — старые ID освобождаются, новые аллоцируются
- Для случаев когда нужна итерация (ECS-like) — отдельная dense-array абстракция

### 5. Extension conflicts

Как в Rust: extension methods доступны только если модуль с `extend` импортирован. При конфликте — fully qualified вызов:

```rust
import unit_utils     // объявляет extend Unit { fn power_level ... }
import combat_utils   // тоже объявляет extend Unit { fn power_level ... }

// hero.power_level()  // ОШИБКА: ambiguous
unit_utils.power_level(hero)     // OK
combat_utils.power_level(hero)   // OK
```

---

## Формальная грамматика Glass

```ebnf
(* === Top level === *)
module          = definition* ;
definition      = struct_def | enum_def | fn_def | const_def
                | extend_def | external_def | import_def ;

(* === Imports === *)
import_def      = "import" module_path [ "{" import_items "}" ]     (* simple/selective *)
                | "import" module_path "{" grouped_item { "," grouped_item } "}" ;  (* grouped *)
module_path     = LOWER_IDENT { "/" LOWER_IDENT } ;
import_items    = import_item { "," import_item } ;
import_item     = ( UPPER_IDENT | LOWER_IDENT | "self" ) [ "as" IDENT ] ;
grouped_item    = LOWER_IDENT [ "{" import_items "}" ] ;            (* sub-module with optional selective items *)

(* === Struct — single-variant, no tag === *)
struct_def      = [ "pub" ] "struct" UPPER_IDENT [ type_params ] "{" named_field { "," named_field } "}" ;

(* === Enum — multiple variants, with tag === *)
enum_def        = [ "pub" ] "enum" UPPER_IDENT [ type_params ] "{" variant { variant } "}" ;
variant         = UPPER_IDENT                                      (* nullary: Lobby *)
                | UPPER_IDENT "(" positional_field { "," positional_field } ")"  (* tuple: Ok(T) *)
                | UPPER_IDENT "{" named_field { "," named_field } "}" ;         (* record: Playing { wave: Int } *)

type_params     = "(" UPPER_IDENT { "," UPPER_IDENT } ")" ;
named_field     = LOWER_IDENT ":" type_expr ;
positional_field = type_expr ;

(* === Functions === *)
fn_def          = [ "pub" ] [ "local" ] "fn" LOWER_IDENT "(" [ params ] ")" [ "->" type_expr ] block ;
params          = param { "," param } ;
param           = LOWER_IDENT ":" type_expr ;
block           = "{" expr "}" ;

(* === Constants === *)
const_def       = [ "pub" ] "const" UPPER_IDENT [ ":" type_expr ] "=" expr ;

(* === Extensions === *)
extend_def      = "extend" UPPER_IDENT [ type_params ] "{" fn_def* "}" ;

(* === External bindings === *)
external_def    = "@external(" STRING "," STRING ")"
                  [ "pub" ] "fn" LOWER_IDENT "(" [ params ] ")" [ "->" type_expr ] ;

(* === Expressions === *)
expr            = let_expr | case_expr | if_expr | pipe_expr | lambda_expr | block_expr ;
let_expr        = "let" pattern [ ":" type_expr ] "=" expr expr ;
case_expr       = "case" expr "{" case_arm { case_arm } "}" ;
case_arm        = pattern [ "if" expr ] "->" expr ;
if_expr         = "if" expr block [ "else" ( if_expr | block ) ] ;
block_expr      = "{" expr { expr } "}" ;
lambda_expr     = "fn" "(" [ params ] ")" [ "->" type_expr ] block ;

(* Pipe + binary ops (by precedence, low to high) *)
pipe_expr       = or_expr { "|>" or_expr } ;
or_expr         = and_expr { "||" and_expr } ;
and_expr        = cmp_expr { "&&" cmp_expr } ;
cmp_expr        = add_expr { ( "==" | "!=" | "<" | ">" | "<=" | ">=" ) add_expr } ;
add_expr        = mul_expr { ( "+" | "-" | "<>" ) mul_expr } ;
mul_expr        = unary_expr { ( "*" | "/" | "%" ) unary_expr } ;
unary_expr      = [ "!" | "-" ] call_expr ;
call_expr       = field_expr [ "(" [ args ] ")" ] ;
field_expr      = primary { "." LOWER_IDENT [ "(" [ args ] ")" ] } ;
args            = expr { "," expr } ;

(* === Primary expressions === *)
primary         = LOWER_IDENT                                      (* variable *)
                | ctor_name "(" args ")"                           (* positional constructor: Option::Some(42) *)
                | ctor_name "{" brace_fields "}"                   (* named constructor: Model { x, y: 1 } *)
                | ctor_name "{" ".." expr { "," LOWER_IDENT ":" expr } "}"  (* record update *)
                | ctor_name                                        (* nullary constructor: Phase::Lobby *)
                | INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL
                | RAWCODE_LITERAL                                  (* 'hfoo' — JASS raw codes *)
                | "True" | "False"
                | "#(" [ expr { "," expr } ] ")"                   (* tuple *)
                | "[" [ expr { "," expr } ] "]"                    (* list *)
                | "[" expr "|" expr "]"                            (* list cons: [head | tail] *)
                | "(" expr ")"                                     (* grouping *)
                | "clone" "(" expr ")"                             (* explicit clone *)
                | "todo" [ "(" STRING ")" ]                        (* placeholder *) ;

ctor_name       = UPPER_IDENT [ "::" UPPER_IDENT ] ;              (* Lobby or Phase::Lobby *)
brace_fields    = brace_field { "," brace_field } ;
brace_field     = LOWER_IDENT ":" expr                             (* named: x: 42 *)
                | LOWER_IDENT ;                                    (* shorthand: x means x: x *)

(* === Patterns === *)
pattern         = "_"                                              (* discard *)
                | LOWER_IDENT                                      (* variable binding *)
                | INT_LITERAL | FLOAT_LITERAL | STRING_LITERAL     (* literal *)
                | "True" | "False"
                | ctor_name [ "(" pattern { "," pattern } ")" ]    (* positional: Option::Some(x) *)
                | ctor_name "{" field_pat { "," field_pat } [ "," ".." ] "}"  (* named: Phase::Playing { wave, .. } *)
                | "#(" pattern { "," pattern } ")"                 (* tuple *)
                | "[" [ pattern { "," pattern } [ "|" pattern ] ] "]"  (* list *)
                | pattern "|" pattern                              (* or pattern *)
                | pattern "as" LOWER_IDENT ;                       (* alias *)

field_pat       = LOWER_IDENT [ "as" LOWER_IDENT ] ;              (* field or field as binding *)

(* === Type expressions === *)
type_expr       = UPPER_IDENT [ "(" type_expr { "," type_expr } ")" ]
                | "fn" "(" [ type_expr { "," type_expr } ] ")" "->" type_expr
                | "#(" type_expr { "," type_expr } ")" ;

(* === Tokens === *)
LOWER_IDENT     = [a-z_][a-zA-Z0-9_]* ;
UPPER_IDENT     = [A-Z][a-zA-Z0-9_]* ;
INT_LITERAL     = [0-9]+ | "0x" [0-9a-fA-F]+ ;
FLOAT_LITERAL   = [0-9]+ "." [0-9]+ ;
STRING_LITERAL  = '"' ( [^"\\] | '\\' . )* '"' ;
RAWCODE_LITERAL = "'" [a-zA-Z0-9]{4} "'" ;
COMMENT         = "//" [^\n]* ;
```

### Ключевые синтаксические решения

- **`struct` vs `enum`** — struct: один вариант, без tag, без повторения имени. Enum: несколько вариантов, с tag.
- **`Type::Variant`** — конструкторы enum квалифицированные (как в Rust). Не протекают в scope. `Phase::Lobby`, `Option::Some(x)`.
- **`{}` для именованных полей, `()` для позиционных** — в конструкторах и паттернах единообразно.
- **Shorthand** — `Model { name, age: 18 }` если переменная `name` в scope, не нужно `name: name`.
- **Grouped imports** — `import jass { math { cos, sin, self }, unit, sfx }`.
- **`'hfoo'` rawcode literals** — JASS четырёхсимвольные коды для ID юнитов/абилок.
- **`<>` для конкатенации строк** (как в Gleam).
- **`clone(x)`** — явное клонирование handle. `Borrowed` состояние: не даёт warning "not consumed".
- **`case` с guard** — `pattern if condition -> expr`.
- **List patterns** — `[head | tail]` для деструктуризации.
- **Record update** — `Model { ..old, wave: new_wave }`.
- **No semicolons** — expressions разделяются переводом строки.
- **`local fn`** — desync-safe функции для GetLocalPlayer.

---

## Полный пример программы

```glass
import effect
import int
import jass { math, unit, sfx }

pub enum Phase {
  Lobby
  Playing { wave: Int, lives: Int, gold: Int }
  GameOver { final_wave: Int }
}

pub struct Model {
  phase: Phase,
  tick: Int,
}

pub enum Msg {
  StartGame
  WaveTick
  UnitDied { killer_id: Int, bounty: Int }
}

pub fn init() -> #(Model, List(effect.Effect(Msg))) {
  let model = Model { phase: Phase::Lobby, tick: 0 }
  #(model, [
    effect.display_text(0, "Waiting for players...", 5.0),
    effect.after(5.0, fn() { Msg::StartGame }),
  ])
}

pub fn update(model: Model, msg: Msg) -> #(Model, List(effect.Effect(Msg))) {
  case msg {
    Msg::StartGame -> {
      let new_model = Model {
        phase: Phase::Playing { wave: 1, lives: 20, gold: 100 },
        tick: 0,
      }
      #(new_model, [
        effect.display_text(0, "Wave 1 — Fight!", 5.0),
        effect.after(2.0, fn() { Msg::WaveTick }),
      ])
    }

    Msg::WaveTick -> {
      case model.phase {
        Phase::Playing { wave, lives, gold, .. } -> {
          let new_model = Model {
            phase: Phase::Playing { wave, lives, gold },
            tick: model.tick + 1,
          }
          #(new_model, [
            effect.display_text(0, "Tick " <> int.to_string(model.tick), 2.0),
            effect.after(1.5, fn() { Msg::WaveTick }),
          ])
        }
        _ -> #(model, [])
      }
    }

    Msg::UnitDied { bounty, .. } -> {
      case model.phase {
        Phase::Playing { wave, lives, gold, .. } ->
          #(Model { phase: Phase::Playing { wave, lives, gold: gold + bounty }, tick: model.tick }, [])
        _ -> #(model, [])
      }
    }
  }
}
```

---

## TODO (задачи для реализации)

### Milestone 1: Минимальный компилятор (выражения → JASS)

- [ ] **1.1 Настройка проекта** — добавить `logos` в Cargo.toml, настроить структуру модулей (`lexer.rs`, `token.rs`, `ast.rs`, `parser.rs`, `codegen.rs`, `error.rs`). Точка входа: CLI принимает `.glass` файл, выводит `.j` файл.

- [ ] **1.2 Лексер** — реализовать tokenizer на `logos`. Токены: ключевые слова (`fn`, `let`, `case`, `if`, `else`, `type`, `pub`, `import`, `local`, `const`, `extend`, `clone`, `todo`, `True`, `False`), идентификаторы (lower_ident, UPPER_IDENT), литералы (int, float, string, rawcode `'xxxx'`), операторы (`+`, `-`, `*`, `/`, `%`, `==`, `!=`, `<`, `>`, `<=`, `>=`, `&&`, `||`, `!`, `|>`, `<>`, `->`, `..`, `=`), разделители (`(`, `)`, `{`, `}`, `[`, `]`, `#(`, `,`, `:`, `.`, `|`), комментарии `//`. Тесты: каждый тип токена отдельно + полный файл.

- [ ] **1.3 AST** — определить типы AST нод. `Module { definitions }`. `Definition`: `TypeDef`, `FnDef`, `ConstDef`, `ExtendDef`, `ExternalDef`, `ImportDef`. `Expr`: `Let`, `Case`, `If`, `BinOp`, `UnaryOp`, `Call`, `FieldAccess`, `MethodCall`, `Var`, `Constructor`, `RecordUpdate`, `Literal`, `Tuple`, `List`, `Lambda`, `Pipe`, `Block`, `Clone`, `Todo`. `Pattern`: `Var`, `Discard`, `Literal`, `Constructor`, `Tuple`, `List`, `ListCons` (`[h|t]`), `As`. `TypeExpr`: `Named`, `Fn`, `Tuple`. Все ноды имеют `Span` (offset + length) для error reporting.

- [ ] **1.4 Парсер** — recursive descent. Каждая грамматическая конструкция = отдельная функция `parse_X`. Порядок приоритетов операторов: pipe < or < and < cmp < add < mul < unary < call < field. Обработка ошибок: `Result<T, ParseError>` с span и сообщением. Тесты: парсинг каждой конструкции, парсинг полного файла-примера.

- [ ] **1.5 Кодогенерация (базовая)** — трансляция подмножества AST в JASS. На этом этапе: `fn` → `function/endfunction`, `let` → `local + set`, `if/else` → `if/then/elseif/else/endif`, `Int`/`Float`/`Bool`/`String` литералы, бинарные операции, вызовы функций, `return`. Вход: AST. Выход: строка с JASS кодом. Тест: `fn add(a: Int, b: Int) -> Int { a + b }` → валидный JASS.

- [ ] **1.6 E2E тест** — скомпилировать простой файл, прогнать через pjass (JASS syntax checker) для валидации.

### Milestone 2: Типы и pattern matching

- [ ] **2.1 Типы → SoA (Struct of Arrays)** — для каждого `type` определения генерировать: массив на каждое поле, freelist (массив `_free` + `_free_top` integer), `_alloc` функцию (pop из freelist или increment counter), `_dealloc` функцию (push в freelist). Для variants: дополнительный `_tag` массив. Для каждого поля — getter/setter функция. Лимит экземпляров: 8190 (JASS array limit). Тест: создание, доступ к полям, уничтожение.

- [ ] **2.2 Pattern matching → if/elseif** — компиляция `case` выражений. Для variant: загрузить tag, if/elseif цепочка. Для каждого arm: извлечь поля в locals по индексам. Nested patterns: flatten в серию проверок. Wildcard `_`: default ветка. Guard `if condition`: дополнительная проверка после pattern match. Exhaustiveness check (warning, не error — на первом этапе). Тест: case на enum с payload, nested patterns.

- [ ] **2.3 Record update** — `Model(..old, wave: 5)` компилируется в: alloc новый ID, скопировать все поля из old, перезаписать указанные. Для линейных типов: dealloc old. Тест: обновление одного поля, нескольких полей.

- [ ] **2.4 Кортежи (tuples)** — `#(a, b, c)` компилируется в **отдельные переменные** (inline, zero overhead). `#(Int, Float)` → два locals: `_t0_0` integer, `_t0_1` real. Деструктуризация `let #(x, y) = expr` → присвоение из соответствующих переменных. Тест: создание, деструктуризация, передача в функцию.

- [ ] **2.5 Списки (linked list)** — `List(a)` — встроенный generic тип. Реализация: SoA с полями `_head` (значение) и `_tail` (ID следующего или -1). `[]` = -1. `[1, 2, 3]` = цепочка аллокаций. Pattern `[h | t]` = проверка != -1, загрузка head и tail. Мономорфизация: `List(Int)` и `List(Unit)` = разные массивы. Тест: создание, pattern match, list.map.

### Milestone 3: Замыкания и функции высшего порядка

- [ ] **3.1 Замыкания без захвата** — `fn() { expr }` без свободных переменных → обычная JASS функция + integer ID в dispatch table. Вызов: `glass_dispatch_closureN(id)`. Тест: передача callback без захвата, вызов.

- [ ] **3.2 Замыкания с захватом** — анализ свободных переменных в теле лямбды. Генерация SoA struct для каждой уникальной лямбды: поле на каждую captured variable. `alloc` сохраняет captured values. Callback загружает их из массивов по closure ID. Value semantics (capture by value). Тест: лямбда захватывающая одну переменную, две переменные, вложенные замыкания.

- [ ] **3.3 Dispatch таблица для замыканий** — все замыкания одной сигнатуры (`fn() -> Msg`, `fn(Unit) -> Msg` и т.д.) используют общий dispatch: `if closure_type == 0 then call clos_0_run(id) elseif ...`. Closure = пара (type_tag, instance_id). Тест: несколько замыканий одной сигнатуры, вызов по dispatch.

- [ ] **3.4 Pipe operator** — `a |> f(b)` → `f(a, b)`. Парсер уже поддерживает (milestone 1). Кодогенерация: развернуть pipe в вложенные вызовы. `a |> f |> g(x)` → `g(f(a), x)`. Тест: цепочка из 3+ pipe.

### Milestone 4: Elm Architecture Runtime

- [ ] **4.1 Runtime preamble** — генерировать JASS-код runtime при компиляции: global для model ID, msg dispatch function, effect queue (массивы: `_fx_tag`, `_fx_int_0..N`, `_fx_real_0..N`, `_fx_count`), `glass_init` (вызывает user init, процессит начальные эффекты), `glass_send_msg` (сохраняет payload → dispatch → process effects). Runtime встраивается в начало output .j файла.

- [ ] **4.2 init/update компиляция** — распознать `pub fn init()` и `pub fn update(model, msg)` как entry points. `init` должен возвращать `#(Model, List(Effect(Msg)))`. `update` принимает Model + Msg, возвращает `#(Model, List(Effect(Msg)))`. Компилятор генерирует `glass_user_init` и `glass_user_update` + dispatch по Msg tag. Тест: init создаёт модель, update обрабатывает 2 типа сообщений.

- [ ] **4.3 One-shot эффекты** — определить Effect как built-in тип. Варианты: `After(Float, closure)`, `CreateUnit(Player, Int, Float, Float, Float, closure)`, `DisplayText(Player, String, Float)`, `Batch(List(Effect))`, `None`. Компиляция `After`: создать timer, сохранить closure ID через GetHandleId в hashtable, TimerStart с generated callback. Callback: GetExpiredTimer → load closure ID → dispatch → destroy timer. Тест: effect.after(5.0, fn() { Tick }) работает в WC3.

- [ ] **4.4 Subscriptions + reconciliation** — `pub fn subscriptions(model) -> List(Sub(Msg))`. Sub варианты: `Every(Float, closure, key: String)`, `OnEvent(EventType, closure, key: String)`. Runtime хранит `current_subs: HashMap<String, SubState>`. После каждого update: вызвать subscriptions, сравнить ключи. Новые → создать trigger/timer. Удалённые → destroy. Одинаковые → skip. `SubState` хранит handle (timer/trigger) + closure ID. Тест: подписка появляется при смене фазы, исчезает при следующей смене.

### Milestone 5: Линейные типы и безопасность

- [ ] **5.1 Move checker** — после type checking, отдельный pass по AST. Для каждой переменной трекать: alive/moved/partially-moved. Использование moved переменной = ошибка. Branching: если moved в одной ветке if, должно быть moved во всех. Тест: ошибка при двойном использовании, OK при move + clone.

- [ ] **5.2 Auto-cleanup** — для линейных handle: когда переменная выходит из scope без move, вставить cleanup. Маппинг типов → cleanup: `Timer → DestroyTimer + set = null`, `Group → DestroyGroup + set = null`, `Unit → RemoveUnit + set = null` (configurable). Для not-consumed → compiler warning + auto destroy. Тест: функция создаёт timer, не передаёт → сгенерирован DestroyTimer.

- [ ] **5.3 `local fn` checker** — отдельный pass. Внутри `local fn`: запретить вызов handle-creating natives, запретить ForGroup/ForForce, запретить TriggerSleepAction. Разрешить: камера, звук, UI, чтение state. Обычная fn не может вызвать local fn напрямую. Тест: ошибка при CreateUnit внутри local fn.

### Milestone 6: JASS SDK + стандартная библиотека

- [ ] **6.1 Парсер common.j / blizzard.j** — парсить JASS native declarations: `native FuncName takes type1, type2 returns type` и `type typename extends basename`. Построить таблицу: имя → параметры → return type → handle hierarchy. Формат прост, regex-level парсинг достаточен.

- [ ] **6.2 Авто-биндинги** — из таблицы natives сгенерировать Glass `@external` декларации. Маппинг типов: `integer → Int`, `real → Float`, `boolean → Bool`, `string → String`, `handle/подтипы → соответствующий Glass тип`. Классификация: pure (Get*, Is*) vs effectful (Create*, Destroy*, Set*, Remove*). Effectful нельзя вызывать напрямую — только через Effect. Output: файлы `glass/jass/unit.glass`, `glass/jass/timer.glass` и т.д.

- [ ] **6.3 Extension functions для SDK** — `extend Unit { fn x(self) -> Float { get_unit_x(self) } }` и т.д. Стандартная библиотека с ergonomic wrappers. Группировка по доменам: unit, player, timer, trigger, effect, camera, sound.

- [ ] **6.4 Error reporting** — подключить `miette` или `ariadne`. Показывать исходный код с подсветкой ошибки, span, контекст. Типы ошибок: parse error, type mismatch, move-after-use, linear type not consumed, desync-unsafe call in local fn, unknown identifier.

### Milestone 7: Полировка и демо

- [ ] **7.1 Модульная система** — `import jass/unit`, `import my_module/{Foo, bar}`. Каждый `.glass` файл = модуль. Qualified access: `unit.get_x(u)`. Pub/private visibility. Dependency resolution (topological sort). Circular import detection.

- [ ] **7.2 Оптимизации** — dead code elimination (неиспользуемые функции не попадают в output). Constant folding (1 + 2 → 3). Inlining тривиальных функций (getter/setter). Tuple elimination (tuples не аллоцируются, а inline в переменные — уже в 2.4).

- [ ] **7.3 Демо: Tower Defense** — полноценная карта на Glass. Model: фаза игры, волны, жизни, очки. Msg: Tick, UnitDied, TowerBuilt, WaveComplete. Subscriptions: periodic tick, unit death event, build event. Effects: spawn wave, give gold, show text. Цель: доказать что язык работает end-to-end.

---

## Verification (как проверять)

1. **Milestone 1:** `cargo run -- examples/add.glass > out.j && pjass out.j` — валидный JASS
2. **Milestone 2:** custom types + pattern match → корректные массивы и if/elseif в JASS
3. **Milestone 3:** closure test → JASS с dispatch table, ручная проверка корректности
4. **Milestone 4:** загрузить output в WC3 World Editor, запустить карту, проверить что таймер тикает и model обновляется
5. **Milestone 5:** compiler errors на move-after-use и desync-unsafe — unit тесты
6. **Milestone 6:** скомпилировать программу с JASS natives → работает в WC3
7. **Milestone 7:** tower defense карта играбельна
