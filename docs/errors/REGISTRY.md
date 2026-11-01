# Diagnostic code registry

This file is the checked list of all `L####` diagnostic codes. It makes the
ranges in `docs/ARCH.md` Appendix A enforceable. `scripts/check-codes.sh`
reads the table below and fails when:

- a code in `src/` is not in this registry, or is still marked `reserved`;
- a code marked `in use` is not emitted in `src/`, has no page in this
  directory, or has no row in `src/explain.rs`;
- a code falls outside every range in this table (new codes must take a
  reserved range first, then flip to `in use` when first emitted);
- two rows claim the same code or overlapping ranges.

Rules for a new code:

1. Pick the next free code inside the reserved range for your area. If no
   range fits, add a new reserved row first (free space starts at L05xx;
   never reuse a code, and never renumber).
2. In the same change: emit the code, add its page `L####.md`, add its row
   to `src/explain.rs` and to the index in `README.md`, and flip or split
   the registry row so the code is `in use`.
3. Run `scripts/check-codes.sh` and `scripts/check-explain.sh`.

Status is `in use` (the compiler emits every code in the range today) or
`reserved` (planned, not yet emitted). `lust explain` reads this file: it
lists the in-use codes and the reserved ranges, and for a reserved code it
prints "reserved, not yet emitted".

| Code or range | Area | Owner | Status | Notes |
|---|---|---|---|---|
| L0001 | Syntax | lexer, parser | in use | |
| L0002 | Syntax | lexer | in use | Identifier or lifetime name is not in Unicode normal form C (NFC) |
| L0101-L0107 | Names, types, fields, methods, arity, duplicates | check, collect | in use | |
| L0110-L0119 | Type mismatch, inference, traits, operators, casts, literals | check, collect | in use | |
| L0120-L0122 | Break/continue, assignment target, `mut` under a reference | check | in use | |
| L0130 | No `main` function | driver | in use | |
| L0201-L0203 | References in fields, lifetime elision and syntax | collect, parser | in use | |
| L0301-L0312 | Borrow checker: moves, borrows, value lifetimes | borrowck | in use | |
| L0401-L0405 | User macros, attributes, derives, `unsafe`, `async` | parser, collect | in use | |
| L0501 | Modules and imports | W-RESOLVE | reserved | Import cycle does not settle |
| L0502-L0503 | Modules and imports | W-RESOLVE | in use | L0502 ambiguous module file, L0503 module file not found |
| L0504 | Modules and imports | W-RESOLVE | reserved | Generated name changes an earlier resolution |
| L0505 | Modules and imports | W-RESOLVE | in use | Module-path forms lust cannot place yet (source-dependency items work; see L0508) |
| L0506-L0507 | Modules and imports | W-RESOLVE | in use | L0506 unresolved import, L0507 ambiguous glob name used |
| L0508 | Modules and imports | W-RESOLVE | in use | Dependency crate has no lust source, adapter, or shim form yet |
| L0509 | Modules and imports | W-RESOLVE | in use | Glob name whose binding depends on rustc's import order (subset rule) |
| L0510 | Visibility | W-RESOLVE | in use | Item or field is private |
| L0511 | Visibility | W-RESOLVE | in use | Restricted visibility names no enclosing module |
| L0512-L0519 | Visibility | W-RESOLVE | reserved | |
| L0520 | cfg and attributes | W-RESOLVE | in use | Unknown cfg name |
| L0521 | cfg and attributes | W-MACRO | reserved | Unknown attribute |
| L0522 | cfg and attributes | W-MACRO | in use | Known attribute (cfg, repr, path, inline, and kin) is not supported yet (S3b parser reject) |
| L0523 | cfg and attributes | W-MACRO | in use | Unstable cfg predicate (rustc E0658) |
| L0524 | cfg and attributes | W-MACRO | in use | Built-in attribute with a malformed input or on a target Rust does not permit |
| L0525-L0529 | cfg and attributes | W-MACRO | reserved | |
| L0530-L0531 | Crate session | W-RESOLVE | in use | L0530 unsupported edition, L0531 unsupported deny-level lint |
| L0532 | Crate session | W-DRIVER | in use | Refused in a `--sandbox` build (check/sandbox.rs) |
| L0533-L0539 | Crate session | W-DRIVER | reserved | |
| L0540 | Known-value checks | check | in use | Arithmetic operation will overflow (rustc `arithmetic_overflow`) |
| L0541 | Known-value checks | check | in use | Operation will panic at runtime (rustc `unconditional_panic`) |
| L0542 | Known-value checks | W-THREADS | in use | Invalid atomic memory ordering (rustc `invalid_atomic_ordering`) |
| L0543 | Known-value checks | tf-mdrop | in use | `drop` of a `ManuallyDrop` value does nothing (rustc `undropped_manually_drops`) |
| L0544 | Known-value checks | tf-mdrop | in use | `let _ =` on a lock guard (rustc `let_underscore_lock`) |
| L0545 | Known-value checks | tf-mdrop | in use | A codepoint that changes text direction in a comment or literal (rustc `text_direction_codepoint_in_comment`, `_in_literal`) |
| L0546 | Known-value checks | tf-v07 | in use | `mem::discriminant` of a type that is not an enum (rustc `enum_intrinsics_non_enums`) |
| L0547-L0549 | Known-value checks | check | reserved | |
| L0601 | Types and aliases | W-RESOLVE | in use | Type alias cycle |
| L0602 | Types and aliases | W-CORELANG | in use | Type form or type alias is not supported yet (S3b parser reject) |
| L0603 | Types and aliases | fx-scanfa | in use | Unstable type form: a written `!` outside a fn return type (rustc E0658) |
| L0604-L0609 | Types and aliases | W-CORELANG | reserved | |
| L0610-L0613 | Traits and inference | W-TRAITS-CORE | in use | L0610 annotations needed, L0611 selection overflow, L0612 no impl, L0613 impl parameter not known |
| L0614 | Traits and inference | W-TRAITS-CORE | in use | Trait, generic, or where-clause form is not supported yet (S3b parser reject) |
| L0615 | Traits and inference | W-TRAITS-CORE | in use | Overlapping impls (coherence) |
| L0616 | Traits and inference | W-TRAITS-CORE | in use | Orphan-rule violation |
| L0617 | Traits and inference | fx-outlives | in use | A written type needs an outlives bound that does not hold (rustc E0309, E0310, E0491) |
| L0618-L0619 | Traits and inference | W-TRAITS-CORE | reserved | |
| L0620 | Associated items | W-TRAITS-CORE | reserved | L0620 GAT |
| L0621 | Associated items | W-TRAITS-CORE | in use | Associated item, binding, or qualified path is not supported yet (S3b parser reject) |
| L0622 | Associated items | fx-traitdepth | in use | `Self::V` is both an enum variant and an associated type (rustc `ambiguous_associated_items`) |
| L0623-L0629 | Associated items | W-TRAITS-CORE | reserved | |
| L0630-L0631 | Const eval | W-CORELANG | in use | L0630 outside the bounded evaluator, L0631 non-const call in a const context |
| L0632 | Const eval | W-CORELANG | in use | Const or static item, const fn, or const generic form is not supported yet (S3b parser reject) |
| L0633 | Const eval | W-CORELANG | in use | Static or non-structural constant used in a pattern |
| L0634 | Const eval | fx-constbck2 | in use | A const context names a local of the fn around it (rustc E0435) |
| L0635-L0639 | Const eval | W-CORELANG | reserved | |
| L0640 | Std surface | W-STD | in use | Std item is declared but not yet implemented (S5) |
| L0641 | Std surface | W-STD | in use | Float or byte literal, known std type, or format form is not supported yet (S3b) |
| L0642 | Std surface | W-STD | in use | Intrinsic called from a crate that may not use intrinsics (S5) |
| L0643 | Std surface | fx-alias-builtin | in use | A std item of the name may win over or tie with a non-std trait item (rule zero) |
| L0644-L0649 | Std surface | W-STD | reserved | |
| L0650 | Layout | W-CORELANG | in use | `offset_of!` on a default-repr type |
| L0651-L0652 | Layout | W-CORELANG | in use | L0651 reference to a packed field, L0652 unsupported layout form |
| L0653-L0659 | Layout | W-CORELANG | reserved | |
| L0701-L0705 | dyn compatibility | W-TRAITS-CORE | in use | L0701 not dyn compatible (names the offending item), L0702 dyn form not supported yet, L0703 method cannot be called on a trait object, L0704 impl of a trait the dyn type implements already, L0705 object lifetime bound lust cannot check |
| L0706-L0709 | dyn compatibility | W-TRAITS-CORE | reserved | |
| L0710-L0719 | Binders | W-TRAITS-CORE | reserved | L0710 nested higher-ranked binder |
| L0720 | Methods and receivers | W-TRAITS-CORE | in use | L0720 ambiguous method; plain "no such method" stays L0104 |
| L0721 | Methods and receivers | W-TRAITS-CORE | in use | Method turbofish or typed receiver form is not supported yet (S3b) |
| L0722-L0729 | Methods and receivers | W-TRAITS-CORE | reserved | L0722 unsupported receiver type |
| L0730 | Regions and drop check | tf-mdrop | in use | Drop check overflow: a type whose drop check never ends (rustc E0320) |
| L0731 | Regions and drop check | W-BORROW | in use | Explicit lifetime turbofish arg |
| L0732 | Regions and drop check | W-BORROW | reserved | L0732 placeholder region escapes |
| L0733 | Regions and drop check | W-BORROW | in use | A region relation the signature does not give (rustc "lifetime may not live long enough") |
| L0734 | Regions and drop check | W-BORROW | in use | An impl method's lifetimes do not fit the trait method's signature |
| L0735 | Regions and drop check | W-BORROW | in use | A closure with a fixed argument lifetime where a bound is higher-ranked (rustc "not general enough") |
| L0736-L0739 | Regions and drop check | W-BORROW | reserved | |
| L0801-L0803 | Macros | W-MACRO | in use | L0801 expansion caps, L0802 unknown macro, L0803 include file not found |
| L0804 | Macros | W-MACRO | reserved | L0804 proc-macro dependency |
| L0805-L0807 | Macros | W-MACRO | in use | L0805 local ambiguity, L0806 invalid macro call or builtin input, L0807 invalid macro definition |
| L0808 | Macros | W-MACRO | reserved | |
| L0809 | Macros | W-MACRO | in use | Known macro form lust does not support yet (for example `offset_of!`) |
| L0810 | Foreign and shim boundary | W-SHIM | in use | Extern block or foreign item is not supported yet (S3b parser reject) |
| L0811-L0819 | Foreign and shim boundary | W-SHIM | reserved | |
| L0820 | Core syntax forms | W-CORELANG | in use | Core expression, pattern, or statement form is not supported yet (S3b parser reject; range added by S3b because no Appendix A range covers core syntax) |
| L0821-L0829 | Core syntax forms | W-CORELANG | reserved | |
| L9001 | Feature not supported yet | all | in use | Do not add new L9001 sites for planned features; use the ranges above |
