# DVRDesk Copilot Instructions

## Engineering Journal

- Maintain [docs/ENGINEERING_JOURNAL.md](../docs/ENGINEERING_JOURNAL.md) as part of normal implementation work.
- For each meaningful user prompt cycle that results in code intended for commit, PR, or push, append a concise entry under the active unreleased section.
- Each entry should capture:
  - request or problem being addressed
  - rationale for adding, changing, or removing behavior
  - symptoms observed during debugging
  - solution implemented
  - validation performed
- Prefer concise factual language over narrative prose.
- Update the journal before finalizing a code-change session and before any requested commit or push.
- Do not create journal entries for pure exploration with no code changes.

## Repo Notes

- Live TV source filters should use unique `source_name` values from the Channels API.
- The `All Channels` live view is intentionally deduped to match Channels DVR behavior for duplicate tuner feeds.
- Channel logos should use the shared helpers in `src/lib/channelLogos.ts`.