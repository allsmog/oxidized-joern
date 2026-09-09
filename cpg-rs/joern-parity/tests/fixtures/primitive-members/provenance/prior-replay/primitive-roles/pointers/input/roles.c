unsigned long external_ulong(unsigned long value);
unsigned long *external_ulong_ptr(unsigned long *value);
unsigned long int external_ulong_int(unsigned long int value);
_Bool external_bool(_Bool value);
long double external_long_double(long double value);
unsigned long call_ulong(unsigned long value) { return external_ulong(value); }
unsigned long *call_ulong_ptr(unsigned long *value) { return external_ulong_ptr(value); }
unsigned long int call_ulong_int(unsigned long int value) { return external_ulong_int(value); }
_Bool call_bool(_Bool value) { return external_bool(value); }
long double call_long_double(long double value) { return external_long_double(value); }
unsigned long *pointer_local(unsigned long *value) { unsigned long *local = value; return local; }
const unsigned long *const_pointer(const unsigned long *value) { const unsigned long *local = value; return local; }
volatile unsigned long *volatile_pointer(volatile unsigned long *value) { volatile unsigned long *local = value; return local; }
const volatile unsigned long *cv_pointer(const volatile unsigned long *value) { const volatile unsigned long *local = value; return local; }
unsigned long (*global_callback)(unsigned long);
unsigned long callback_local(unsigned long value) { unsigned long (*local_callback)(unsigned long) = global_callback; return local_callback(value); }
unsigned long callback_parameter(unsigned long (*callback)(unsigned long), unsigned long value) { return callback(value); }
void qualifier_objects(int * const p, int * volatile q) { int * const cp = p; int * volatile vq = q; }
