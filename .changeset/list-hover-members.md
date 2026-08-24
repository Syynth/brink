---
"@brink-lang/web": patch
---

Hovering a `LIST` or a list item now shows the full member set — declared order, explicit ordinals (`spare = 5`) and default-active parens preserved, the hovered member bolded. Internally the hover is now assembled from an ordered section-provider table (head line + Markdown blocks), so future per-kind hover content is a one-provider addition; the *Defined in* note moved to the end as a footer.
