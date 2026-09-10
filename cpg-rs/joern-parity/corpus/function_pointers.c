int concrete(int value) { return value; }
int external_fn(int);
int (*global_callback)(int) = concrete;
int local_defined(int value) { int (*fp)(int) = concrete; return fp(value); }
int local_external(int value) { int (*fp)(int) = external_fn; return fp(value); }
int local_declared_only(int value) { int (*fp)(int); return fp(value); }
int call_global(int value) { return global_callback(value); }
int parenthesized_defined(int value) { return (concrete)(value); }
int shadow_external(int (*cb)(int), int value) { int (*external_fn)(int) = cb; return external_fn(value); }
