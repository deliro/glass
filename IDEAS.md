# Glass — Ideas & Backlog

## High Priority
- [ ] В тестах и examples писать код который пройдёт type checker. Сейчас `fn update(model: Int, msg: Int)` — потом сломается. **Делать перед M6 когда type checker будет готов.**

## Mid Priority
- [x] List: linked list vs array? **Linked list — правильный выбор.** O(1) prepend, O(1) pattern match `[h|t]`, естественно для FP. Для dense iteration добавить `Array(a)` позже отдельно.
- [ ] Топологическая сортировка функций в JASS output. Граф вызовов → topo sort → ошибка при циклах. **Делать в M7.**

## Low Priority
- [ ] JASS интерпретатор для runtime-тестов. Варианты: JassBot, свой на Rust. **Не сейчас, snapshot + pjass достаточно.**
- [ ] Чистые эффекты: update возвращает `(Model, List(Effect))`, runtime обрабатывает. **Блокировано type checker. Делать в M6+.**
- [ ] Timer heap queue (один timer + priority queue вместо множества CreateTimer). Экономит handle'ы. **Оптимизация для M7+.**
- [ ] mangle names
