---
schema_version: 1
generated_at: "2026-03-10T00:00:00Z"
templates:
  - kind: template
    id: template.meeting-notes
    name: Meeting Notes
    version: "1.0.0"
    summary: Structured template for capturing meeting minutes
    description: A creation-time template with sections for attendees, agenda, discussion notes, and action items. Uses date and title variables for automatic metadata.
    author: Diaryx Team
    license: PolyForm Shield 1.0.0
    repository: null
    categories:
      - productivity
      - meetings
    tags:
      - meeting
      - notes
      - agenda
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/templates/artifacts/template.meeting-notes/1.0.0/template.json"
      sha256: "0000000000000000000000000000000000000000000000000000000000000000"
      size: 512
      published_at: "2026-03-10T00:00:00Z"
    template_variables:
      - title
      - date
      - time
    preview: |
      # {{title}}
      **Date:** {{date}}
      ## Attendees
      ## Agenda
      ## Notes
      ## Action Items

  - kind: template
    id: template.book-review
    name: Book Review
    version: "1.0.0"
    summary: Template for organizing book reviews and reading notes
    description: Capture key details about a book including author, genre, rating, key takeaways, and personal reflections.
    author: Diaryx Team
    license: PolyForm Shield 1.0.0
    repository: null
    categories:
      - reading
      - reviews
    tags:
      - book
      - review
      - reading
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/templates/artifacts/template.book-review/1.0.0/template.json"
      sha256: "0000000000000000000000000000000000000000000000000000000000000000"
      size: 480
      published_at: "2026-03-10T00:00:00Z"
    template_variables:
      - title
      - date
    preview: |
      # {{title}}
      **Date read:** {{date}}
      ## Details
      ## Key Takeaways
      ## Favorite Quotes
      ## Personal Reflections

  - kind: template
    id: template.project-plan
    name: Project Plan
    version: "1.0.0"
    summary: Template for planning and tracking project milestones
    description: A structured template for project planning with sections for objectives, timeline, tasks, resources, and risks.
    author: Diaryx Team
    license: PolyForm Shield 1.0.0
    repository: null
    categories:
      - productivity
      - planning
    tags:
      - project
      - planning
      - tasks
    icon: null
    screenshots: []
    artifact:
      url: "https://cdn.diaryx.org/templates/artifacts/template.project-plan/1.0.0/template.json"
      sha256: "0000000000000000000000000000000000000000000000000000000000000000"
      size: 560
      published_at: "2026-03-10T00:00:00Z"
    template_variables:
      - title
      - date
    preview: |
      # {{title}}
      **Created:** {{date}}
      ## Objectives
      ## Timeline
      ## Tasks
      ## Resources
      ## Risks
---

# Templates Registry

Curated creation-time templates for Diaryx workspaces.
