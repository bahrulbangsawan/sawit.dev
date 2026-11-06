# Credits

Ideas Sawit took inspiration from, beyond the code and assets whose licences are in `NOTICE` and
each crate's metadata.

- **Live build cursors** (the canvas's agents' pointers and the replay of what they build,
  `crates/app/src/board/presence/`) are inspired by
  [swarm-cursors](https://github.com/DhvanilPatel/swarm-cursors) by Dhvanil Patel, itself a
  tribute to Joel Califa's [Swarm](https://califa.github.io/swarm-site/). Neither repository has a
  licence, so none of their code, cursor art, timings or sounds is used: Sawit takes only the
  general idea (a blueprint of what was built, scripted tasks per element in dependency order,
  then the live result) and implements it on its own in Rust and GPUI, with its own pointer art,
  timings and synthesised sounds.
- The canvas design standard's sources are credited in `skills/sawit-canvas-design/CREDITS.md`.
