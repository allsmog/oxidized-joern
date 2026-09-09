| Case | MEMBER name | CODE | TYPE_FULL_NAME | ORDER |
|---|---|---|---|---:|
| base_qualified_pointer | value | `*value` | `shortunsigned*` | 1 |
| const_unsigned_short | value | `value` | `shortunsigned` | 1 |
| mixed_declarator_shapes | scalar | `scalar` | `shortunsigned` | 1 |
| mixed_declarator_shapes | pointer | `*pointer` | `shortunsigned*` | 2 |
| mixed_declarator_shapes | table | `table[3]` | `shortunsigned[3]` | 3 |
| nonprimitive_scalar_control | item | `item` | `int` | 1 |
| nonprimitive_scalar_control | integer_value | `integer_value` | `int` | 1 |
| nonprimitive_scalar_control | floating_value | `floating_value` | `double` | 2 |
| nonprimitive_scalar_control | struct_value | `struct_value` | `Inner` | 1 |
| nonprimitive_scalar_control | union_value | `union_value` | `unionChoice` | 2 |
| nonprimitive_scalar_control | alias_value | `alias_value` | `Alias` | 3 |
| ordinary_numeric_control | integer_value | `integer_value` | `int` | 1 |
| ordinary_numeric_control | floating_value | `floating_value` | `double` | 2 |
| pointer_qualified_pointer | value | `* const value` | `shortunsigned*` | 1 |
| short_int_unsigned | value | `value` | `short unsigned int` | 1 |
| short_unsigned | value | `value` | `shortunsigned` | 1 |
| signed_char | value | `value` | `signedchar` | 1 |
| signed_short | value | `value` | `shortsigned` | 1 |
| tiny_fixedtables_include | op | `op` | `unsigned char` | 1 |
| tiny_fixedtables_include | bits | `bits` | `unsigned char` | 2 |
| tiny_fixedtables_include | value | `value` | `shortunsigned` | 3 |
| tiny_fixedtables_include | len | `*len` | `Code*` | 1 |
| tiny_fixedtables_include | dist | `*dist` | `Code*` | 2 |
| tiny_fixedtables_inline | op | `op` | `unsigned char` | 1 |
| tiny_fixedtables_inline | bits | `bits` | `unsigned char` | 2 |
| tiny_fixedtables_inline | value | `value` | `shortunsigned` | 3 |
| tiny_fixedtables_inline | len | `*len` | `Code*` | 1 |
| tiny_fixedtables_inline | dist | `*dist` | `Code*` | 2 |
| unsigned_char | value | `value` | `unsigned char` | 1 |
| unsigned_long_long_int | value | `value` | `longlong unsigned int` | 1 |
| unsigned_short | value | `value` | `shortunsigned` | 1 |
| unsigned_short_array | value | `value[3]` | `shortunsigned[3]` | 1 |
| unsigned_short_int | value | `value` | `short unsigned int` | 1 |
| unsigned_short_pointer | value | `*value` | `shortunsigned*` | 1 |
| volatile_unsigned_short | value | `value` | `volatile shortunsigned` | 1 |

Observed pinned Joern MEMBER properties only. Complete graphs and full ancestor records are bound in live-member-facts.json. No Rust parity or generalized renderer claim.
