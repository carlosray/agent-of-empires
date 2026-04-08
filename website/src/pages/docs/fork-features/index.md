---
layout: ../../../layouts/Docs.astro
title: Fork Features
description: Fork-only features and fixes that this Agent of Empires fork documents on top of upstream.
---

This section documents the behavior that exists in this fork on top of upstream [`njbrake/agent-of-empires`](https://github.com/njbrake/agent-of-empires).

Treat it as the fork changelog for product behavior:

- every fork-only feature or fix gets its own page
- the index stays current as new fork changes are added
- pages can be kept even if a change later moves upstream, with a note explaining the transition

## Current Fork-Only Changes

- [Git branch labels for all git sessions](/docs/fork-features/git-branch-display/)
- [Terminal tab titles on attach](/docs/fork-features/terminal-tab-title/)

## How To Extend This Section

When this fork adds another feature or behavior change on top of upstream:

1. Create a new page in `docs/fork-features/`
2. Add the entry to this index
3. Mirror the page into `website/src/pages/docs/fork-features/`
4. Add the page to `website/src/data/docsNav.ts`

The goal is simple: if a behavior differs from upstream, it should be documented here.
