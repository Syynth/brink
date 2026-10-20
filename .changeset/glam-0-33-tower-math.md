---
"@brink-lang/web": patch
---

The vector/matrix/quaternion value kinds now compute through glam 0.33
(was 0.32), the version Bevy 0.20 uses. Operation semantics are
unchanged, and `lerp` keeps exactly the same formula. However, glam
0.33's SIMD back-ends use fused multiply-add where the target supports
it, and its scalar path divides instead of multiplying by a reciprocal.
So a `dot`, a matrix × vector or a quaternion × vector result can
differ from 0.32's in the last bit.
