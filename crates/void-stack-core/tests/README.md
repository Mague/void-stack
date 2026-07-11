# void-stack-core tests

## Data-dir isolation (required for any test touching central state)

Several core APIs persist artifacts under the **central data dir**
(`<data_local_dir>/void-stack/` — indexes, contracts caches, stats,
briefings). A test that calls `open_meta_db`, `project_contracts`,
`index_project`, `save_stats`, etc. with a fixture project would
otherwise write into the **real user data dir** and leave orphan
directories behind (`contracts-test-<pid>`, `deadcode-fixture-<pid>`,
... — exactly what `void doctor` reports as OrphanIndex).

The base directory honors the `VOID_STACK_DATA_DIR` env var (see
`global_config::data_base_dir`). Two ways to use it:

- **Unit tests (inside `src/`)**: call `crate::isolate_test_data_dir()`
  as the first statement of the test. It points `VOID_STACK_DATA_DIR` at
  one shared per-process tempdir; repeated calls converge on the same
  directory, so parallel tests don't race.

- **Integration tests (this directory)**: replicate the same pattern
  locally — the helper is `pub(crate)`:

  ```rust
  use std::sync::OnceLock;

  fn isolate_data_dir() {
      static DIR: OnceLock<tempfile::TempDir> = OnceLock::new();
      let dir = DIR.get_or_init(|| tempfile::tempdir().unwrap());
      // SAFETY: every caller sets the same value.
      unsafe { std::env::set_var("VOID_STACK_DATA_DIR", dir.path()) };
  }
  ```

Rule of thumb: if your test constructs a `Project` whose `name` is a
fixture (anything not registered in a real config), isolate the data dir
first. Naming convention for fixtures: `<area>-fixture-<pid>` or
`<area>-test-<pid>` — `void doctor --fix` recognizes those patterns and
offers to delete any leaked leftovers in one batch.

## Other conventions

- Filesystem fixtures use `tempfile::tempdir()`; git fixtures configure
  `user.email`/`user.name` and `commit.gpgsign false`.
- Structural graphs built for fixtures live inside the fixture dir
  (`.void-stack/structural.db`) and vanish with the tempdir — no
  isolation needed for those.
- Run everything with `cargo test -p void-stack-core --all-features`
  (CI uses the same flags plus `cargo fmt --check` and
  `clippy -D warnings`).
