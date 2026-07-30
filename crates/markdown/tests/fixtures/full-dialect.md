---
title: Contract fixture
tags:
  - project/demo
  - rust
aliases: [Sample note, 样例]
rating: 5
active: true
nested:
  owner: ignored
anchored: &shared ignored
alias_value: *shared
custom: !secret ignored
---
# 标题 😀

Paragraph with *emphasis*, **strong**, ~~strike~~, <https://example.test/angle>,
https://example.test/literal, [relative](notes/relative.md), ![image](asset.png),
[[Target note#Section|Alias]], ![[asset.png]], and #project/demo. ^paragraph-id

- [x] completed
- [ ] pending with [[#Local heading]]

> quoted evidence

```rust
let inert = "[[not-a-link]] #not-a-tag";
```

| left | right |
| --- | ---: |
| one | two |

^table-anchor

<script>never_execute()</script>
