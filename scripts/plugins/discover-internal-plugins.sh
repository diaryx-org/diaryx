#!/usr/bin/env bash
set -euo pipefail

cargo metadata --no-deps --format-version 1 | jq -c '
  [
    .packages[]
    | select(
        any(.targets[]; (.kind | index("cdylib")) != null)
        and any(.dependencies[]; .name == "extism-pdk")
      )
    | {
        crate: .name,
        manifestPath: .manifest_path,
        target: (
          .targets[]
          | select((.kind | index("cdylib")) != null)
          | .name
        )
      }
  ]
'
