---
title: ROADMAP
description: The plan for future Diaryx features
author: adammharris
created: 2025-12-05T12:06:55-07:00
updated: 2026-06-07T23:46:03-07:00
part_of: '[Diaryx](/Diaryx.md)'
link_of:
- '[Diaryx](/Diaryx.md)'
- '[Diaryx](/diaryx.md)'
attachments: []
link: '[ROADMAP](/ROADMAP.md)'
---
# ROADMAP

Diaryx is currently at version v1.6.0. However, recently the vision for Diaryx has shifted from journaling to archiving, so it is considered alpha-level software at the moment.

The next immediate goal is designing opaque identifiers for Diaryx workspaces, and the files inside.

The design:

`ark:99999/dxBBBBBBC/BBBBC[.<FILE>][?<QUERY>][#<CALLOUT>]`

Eventually Diaryx as an organization will register for a NAAN, which will replace `99999`. `dx` is the “shoulder” of the ID, which allows for changing the ID format in the future if needed. Each capital `B` is a betanumeric character from this alphabet to reduce ambiguity:

`b c d f g h j k m n p q r s t v w x y z 2 3 4 5 6 7 8 9`

Each capital `C` is a checksum character calculated from the previous ID sequence, to capture transcription errors.

`.<FILE>` is a way of accessing different versions of the same file. `?<QUERY>` is a way of accessing metadata for a file. `#<CALLOUT>` is a client-only method of highlighting a specific portion of the content and isn’t actually used by the server.

ARKs typically reserve `?info` and `?json`, as well as `??`. I was thinking I would reserve these values alongside `content` to have special query functionality rather than mapping literally to frontmatter metadata key names. To access a literal `info` or other reserved-name frontmatter key, I was thinking maybe a namespace like `?meta=info` or perhaps a leading dot like `?.info`.

I believe 6 characters to identify a workspace is enough for worldwide Diaryx usage (28^6 = 481,890,304), and 4 characters should be plenty of files allowed for a workspace (28^4 = 614,656). If usage somehow exceeds this, I can change the `dx` shoulder to make a new ID format that allows for more room.
