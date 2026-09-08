int unused_named(int value);
int unused_unnamed(int);
void unused_void(void);
char *external_ptr(const char *value, int count);
int external_variadic(const char *format, ...);
int later(int value);
int external_named(int value);
int external_named(int value);
int uses_named(int value) { return external_named(value); }
int uses_unknown(int value) { return undeclared(value); }
int uses_zero(void) { return undeclared_zero(); }
int uses_many(int value) { return undeclared_many(value, value, value); }
char *uses_ptr(char *value) { return external_ptr(value, 2); }
int uses_varargs(char *value) { return external_variadic(value, 1, 2); }
int uses_later(int value) { return later(value); }
int later(int value) { return value; }
int uses_function_pointer(int (*callback)(int), int value) { return callback(value); }
