# Glass

An **experimental** functional language with Gleam and Rust inspired syntax that compiles to [JASS](https://en.wikipedia.org/wiki/Warcraft_III:_Reign_of_Chaos#Modding).

Warning: this project is vibe-coded and in early development.

## Key Features

* Type safety
* Sum types
* Immutability
* Elm-like architecture: state is updated only through a pure `update` function that takes the old state and returns a new state, optionally producing side effects
* Handles are [linear types](https://en.wikipedia.org/wiki/Substructural_type_system): leak-free
