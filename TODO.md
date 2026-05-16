# TODO: Remaining Improvements and Refactors

The following items should be addressed in upcoming refactors:

- True concurrent downloads respecting `--limit`
  * Currently each download drains the task set immediately, resulting in sequential execution.
- Support top-level media messages when no `--forum` filter is provided
  * Only reply-header (forum) messages are processed today.
- Fix typo in builder name
  * `DownlaoderBuilder` → `DownloaderBuilder` for clarity.
- Make `--limit` optional with a sensible default (e.g. 1)
  * Avoid forcing users to specify the flag on every run.
- Replace panic-inducing `unwrap()`/`expect()` calls
  * Use graceful error handling and user-friendly messages instead of crashing.
- Swap blocking filesystem calls in async context
  * Replace `path.exists()` and `std::fs::metadata()` with Tokio's async equivalents.
- Remove unnecessary per-chunk `file.flush()` calls
  * Rely on buffered I/O for better performance.
- Per-file error recovery
  * Move away from all-or-nothing error handling so one failure doesn't abort the entire session.
- Standardize and secure session file storage
  * Use XDG or OS-specific data dirs, allow `--session-file` or env override, enforce owner-only permissions or encryption.
- Proper pagination for forum topics
  * Don't invoke `GetForumTopics` with `limit = 0`; implement paging or set a positive limit.
- Decouple downloader core from terminal output
  * Emit progress events (or use a callback/trait) instead of using `MultiProgress` directly.
- Versioning
  * After separating progress reporting from core logic (a breaking change), bump the semantic version (e.g. major version increment).