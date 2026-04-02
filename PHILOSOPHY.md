---
title: PHILOSOPHY
part_of: '[Diaryx](/Diaryx.md)'
updated: 2026-03-29T19:58:13-06:00
audience:
- public
- agents
- developers
link_of:
- '[Diaryx](/Diaryx.md)'
link: '[PHILOSOPHY](/PHILOSOPHY.md)'
---
# PHILOSOPHY

In a sentence, the Diaryx philosophy is: *Readable, self-describing files should be usable and accessible for all writing purposes.* If you are familiar with local-first software, you can skip down to the “Self-Describing” section to see Diaryx-specific innovations.

## Readable, Accessible, and Usable

Diaryx uses UTF-8 plain text files with LF line endings. This is a universal standard understood by nearly every computer in the world. Whether it is an ancient piece of plastic from the ‘60s, or smart glasses from 2060, you should be able to read your files, regardless of what software or hardware you have available to you.

However, plain text alone is not enough. Users need their text to have attributes such as *italic* or **bold**. This is an important aspect of being *usable* and *readable*.

Giving semantic meaning to plain text is a long debated issue. The primary standard in this space is Markdown, a syntax first specified by John Gruber in 2002. Diaryx attempts to be both compliant with modern markdown standards, and to build on top of this statement by John Gruber:

> The overriding design goal for Markdown’s formatting syntax is to make it as readable as possible. The idea is that a Markdown-formatted document should be publishable as-is, as plain text, without looking like it’s been marked up with tags or formatting instructions. While Markdown’s syntax has been influenced by several existing text-to-HTML filters, the single biggest source of inspiration for Markdown’s syntax is the format of plain text email.

A less-common but still prevalent standard for Markdown files is to have “structured” and “unstructured” sections. Structured syntaxes such as JSON, TOML, and YAML are used to assist machines in parsing and understanding data. Markdown files with “frontmatter” often use YAML, which is designed to be both readable by humans and parseable by machines.

## Self-Describing

Diaryx’s main innovation in the admittedly overexplored world of Markdown editors is twofold:

- **Diaryx attempts to abstract away markdown syntax entirely.** It uses the TipTap editor’s Markdown extension so that users can use a normal rich text editor instead of needing to memorize Markdown syntax. That way, users work natively with a portable, readable file format without having to think about it or make compromises.
- **Diaryx embeds all metadata in the files themselves**—no separate config files. Your files are all you need. Files connect to each other in a `contents` / `part_of` hierarchy, so you can even use them independent of a filesystem.

The recognized properties in Diaryx are carefully considered and given useful functionality:

- `title`: Kept in sync with the filename and the first H1, if present.
- `link`: the canonical way to link to the current file.
- `links`: explicit outbound links declared by the current file.
- `link_of`: explicit backlinks from files that declare links to the current file.
- `contents`: a list of files considered as “belonging” to the current one.
- `part_of`: the reverse of `contents`.
- `created` / `updated`: automatically set and kept in sync and used as a source of truth for time information.
- `attachments`: declared binary files linked to by the current file.
- `audience`: intended audience(s) for the current file.

## Unimplemented

~~Diaryx still doesn’t match its own philosophy perfectly. Some gaps that are intended to be implemented in the near future:~~ Now implemented in part!

### Links

Diaryx now supports a singular `link` property that declares the canonical way to link to a file. By default, this should be a Markdown workspace-root-relative link, such as `[My File](</link/to/My File.md>)`, and it should resolve back to the file itself.

Diaryx also supports explicit frontmatter-declared `links` and `link_of` properties for non-structural file-to-file relationships. `links` declares outbound links, and `link_of` declares backlinks. Validation checks that outbound links resolve, warns when backlinks are missing or stale, and can self-heal those backlink lists on a best-effort basis.

What remains unimplemented is treating all body-content links as part of that same declared graph automatically. The canonical metadata model is now in place, but body-link extraction and reconciliation are still follow-up work.

### Attachments

Right now, the `attachments` property links directly to binary (non-markdown) files. But each attachment should have its own “attachment note” so that each binary file’s corresponding attachment note can hold relevant information/metadata about that file. This file would contain a singular `attachment` property linking to the actual file (similar to the `link` property). `attachments` properties would link to the attachment note instead to the file directly. Links/embeds in the body content would still link to the file directly, using the attachment note’s `attachment` property as a source of truth for validation. Each attachment note could also contain an `attachment_of` property to backlink to all files linking to the attachment.
