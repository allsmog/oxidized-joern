int external_empty();
int external_two(int, char *);
int external_multi_a(int a), external_multi_b(char *b);
int external_variadic_int(int value, ...);
int local_prototype(int value) { extern int scoped(int arg); return scoped(value); }
int parenthesized_callee(int value) { return (unknown_paren)(value); }
int pointer_local(int value) { int (*fp)(int) = external_multi_a; return fp(value); }
int vararg_definition(int value, ...) { return value; }
int use_variadic_int(int x) { return external_variadic_int(x, 2); }
int unnamed_definition(int, char *) { return 0; }
