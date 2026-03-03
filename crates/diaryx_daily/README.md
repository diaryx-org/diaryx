# diaryx_daily

Shared daily-entry domain logic used by Daily plugin implementations.

This crate is host-agnostic and contains:

- date parsing (`today`, `yesterday`, `YYYY-MM-DD`, natural language)
- canonical daily path helpers (`YYYY/MM/YYYY-MM-DD.md`)
- index filename helpers (`YYYY_index.md`, `YYYY_month.md`)
- template rendering helpers for daily entry content
- plugin-owned daily config data model
