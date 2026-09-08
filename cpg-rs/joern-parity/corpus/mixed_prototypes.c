int (paren_first)(int value), ordinary_second(int value);
int ordinary_first(int value), (paren_second)(int value);
#define MIXED_API extern
MIXED_API int (macro_paren)(int value), macro_ordinary(int value);
int mixed_prototype_caller(int value) { return paren_first(value) + ordinary_second(value) + ordinary_first(value) + paren_second(value) + macro_paren(value) + macro_ordinary(value); }
