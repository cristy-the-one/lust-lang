# Diagnostic codes

Every error lust prints has a stable `L####` code. Run `lust explain L0301` to print
the long-form text for one code, or `lust explain` to list all codes. Each page has a
wrong example that fails with that code and a fixed example that passes.
`scripts/check-explain.sh` checks every example against lust and rustc.
[REGISTRY.md](REGISTRY.md) is the checked list of in-use and reserved code
ranges; `scripts/check-codes.sh` enforces it. Read it before you add a code.

| Code | Meaning |
|---|---|
| [L0001](L0001.md) | syntax error |
| [L0002](L0002.md) | name is not in Unicode normal form C (NFC) |
| [L0101](L0101.md) | name not found |
| [L0102](L0102.md) | type not found |
| [L0103](L0103.md) | no such field |
| [L0104](L0104.md) | no such method, variant, or associated function |
| [L0105](L0105.md) | wrong number of arguments |
| [L0106](L0106.md) | duplicate definition |
| [L0107](L0107.md) | missing field |
| [L0110](L0110.md) | mismatched types |
| [L0111](L0111.md) | type cannot be inferred |
| [L0112](L0112.md) | trait is not implemented |
| [L0113](L0113.md) | operator cannot be applied |
| [L0114](L0114.md) | value is not callable |
| [L0115](L0115.md) | match is not exhaustive |
| [L0116](L0116.md) | invalid cast |
| [L0117](L0117.md) | recursive type has infinite size |
| [L0118](L0118.md) | integer literal out of range |
| [L0119](L0119.md) | lower range bound is greater than the upper bound |
| [L0120](L0120.md) | break or continue outside a loop |
| [L0121](L0121.md) | invalid assignment target |
| [L0122](L0122.md) | `mut` binding under a reference |
| [L0130](L0130.md) | no `main` function |
| [L0201](L0201.md) | a reference in a field needs a lifetime |
| [L0202](L0202.md) | lifetime elision is ambiguous |
| [L0203](L0203.md) | lifetime name or form not supported |
| [L0301](L0301.md) | use of moved value |
| [L0302](L0302.md) | cannot assign to an immutable variable |
| [L0303](L0303.md) | conflicting borrows |
| [L0304](L0304.md) | two mutable borrows at the same time |
| [L0305](L0305.md) | cannot move out of a reference or an index |
| [L0306](L0306.md) | value does not live long enough |
| [L0307](L0307.md) | cannot return a reference to a local value |
| [L0308](L0308.md) | cannot borrow as mutable |
| [L0309](L0309.md) | cannot assign through a shared reference |
| [L0310](L0310.md) | value used before it is assigned |
| [L0311](L0311.md) | cannot move a borrowed value |
| [L0312](L0312.md) | cannot assign while borrowed |
| [L0401](L0401.md) | this macro call has no expanded form yet |
| [L0402](L0402.md) | unsupported attribute |
| [L0403](L0403.md) | cannot derive this trait |
| [L0404](L0404.md) | `unsafe` is not supported |
| [L0405](L0405.md) | `async` is not supported |
| [L0502](L0502.md) | ambiguous module file |
| [L0503](L0503.md) | module file not found |
| [L0505](L0505.md) | this module path form is not supported yet |
| [L0506](L0506.md) | unresolved import |
| [L0507](L0507.md) | ambiguous name from glob imports |
| [L0508](L0508.md) | dependency crate has no lust form yet |
| [L0509](L0509.md) | glob name whose binding depends on rustc's import order (subset rule) |
| [L0510](L0510.md) | item is private |
| [L0511](L0511.md) | visibility names no enclosing module |
| [L0520](L0520.md) | unknown cfg name |
| [L0530](L0530.md) | unsupported edition |
| [L0531](L0531.md) | unsupported deny-level lint |
| [L0532](L0532.md) | refused in a sandboxed build |
| [L0522](L0522.md) | this attribute is not supported yet |
| [L0523](L0523.md) | unstable cfg predicate |
| [L0524](L0524.md) | attribute used wrongly |
| [L0540](L0540.md) | arithmetic operation will overflow |
| [L0541](L0541.md) | operation will panic at runtime |
| [L0542](L0542.md) | invalid atomic memory ordering |
| [L0543](L0543.md) | `drop` of a `ManuallyDrop` does nothing |
| [L0544](L0544.md) | non-binding let on a synchronization lock |
| [L0545](L0545.md) | text direction codepoint in a comment or literal |
| [L0546](L0546.md) | `mem::discriminant` of a type that is not an enum |
| [L0601](L0601.md) | type alias cycle |
| [L0602](L0602.md) | this type form is not supported yet |
| [L0603](L0603.md) | the `!` type is unstable outside a function return type |
| [L0610](L0610.md) | type annotations needed |
| [L0611](L0611.md) | trait selection overflow |
| [L0612](L0612.md) | no impl of the trait for this type |
| [L0613](L0613.md) | an impl parameter is not known from the trait inputs |
| [L0614](L0614.md) | traits and generics are not supported yet |
| [L0614](L0614.md) | this trait or generic form is not supported yet |
| [L0615](L0615.md) | overlapping impls |
| [L0616](L0616.md) | an impl that the orphan rule forbids |
| [L0617](L0617.md) | a type here needs an outlives bound that does not hold |
| [L0621](L0621.md) | associated items are not supported yet |
| [L0621](L0621.md) | associated consts are not supported yet |
| [L0622](L0622.md) | `Self::V` is both a variant and an associated type |
| [L0630](L0630.md) | outside the bounded const evaluator |
| [L0631](L0631.md) | non-const call in a const context |
| [L0632](L0632.md) | const and static items are not supported yet |
| [L0633](L0633.md) | this constant cannot be used in a pattern |
| [L0634](L0634.md) | attempt to use a non-constant value in a constant |
| [L0640](L0640.md) | this std item is declared but not yet implemented |
| [L0641](L0641.md) | this literal or std form is not supported yet |
| [L0642](L0642.md) | intrinsics can only be called from lust's shipped library crates |
| [L0643](L0643.md) | a std item of this name may win over the trait item here |
| [L0650](L0650.md) | `offset_of!` on a default-repr type |
| [L0651](L0651.md) | reference to a packed field |
| [L0652](L0652.md) | unsupported layout form |
| [L0701](L0701.md) | the trait is not dyn compatible |
| [L0702](L0702.md) | this dyn form is not supported yet |
| [L0703](L0703.md) | this method cannot be called on a trait object |
| [L0704](L0704.md) | the dyn type implements this trait already |
| [L0705](L0705.md) | lust cannot prove this dyn lifetime bound |
| [L0720](L0720.md) | ambiguous method |
| [L0721](L0721.md) | this method or receiver form is not supported yet |
| [L0721](L0721.md) | this method form is not supported yet |
| [L0730](L0730.md) | overflow while adding drop-check rules |
| [L0731](L0731.md) | explicit lifetime arguments on a fn |
| [L0733](L0733.md) | lifetime may not live long enough |
| [L0734](L0734.md) | lifetime mismatch with the trait method |
| [L0735](L0735.md) | closure is not general enough |
| [L0801](L0801.md) | macro expansion hit a limit |
| [L0802](L0802.md) | no macro with this name is in scope |
| [L0803](L0803.md) | include file not found |
| [L0805](L0805.md) | local ambiguity in a macro call |
| [L0806](L0806.md) | this macro call does not fit |
| [L0807](L0807.md) | this macro definition is not valid |
| [L0809](L0809.md) | this macro form is not supported yet |
| [L0810](L0810.md) | extern blocks are not supported yet |
| [L0820](L0820.md) | this syntax form is not supported yet |
| [L9001](L9001.md) | feature is not supported yet |
