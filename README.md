# Factory Forge

A 2D factory automation prototype built with data-oriented design:
all belts and items live in flat parallel arrays (`src/belts.rs`),
the simulation is a fixed-timestep two-pass update (`src/sim.rs`),
and rendering interpolates between ticks for buttery-smooth motion
(`src/render.rs`).

## Run

```sh
cargo run --release
```

## Controls

- **WASD / arrows** — pan camera
- **Mouse wheel** — zoom

## Architecture

- `belts.rs` — SoA storage: parallel arrays for item type / belt / lane /
  distance, intrusive linked lists per belt lane, free-list slot recycling.
- `grid.rs` — flat `y * width + x` tile array pointing at belt slots.
- `sim.rs` — pass 1 advances items clamped behind the item ahead;
  pass 2 transfers head items across belt boundaries.
- `render.rs` — pooled sprites mirror item slots; positions are lerped
  using the fixed-timestep overstep fraction.
