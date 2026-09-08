int function(int x) { return x; }
int (*uninitialized_callback)(int);
int global_uninitialized(int x) { return uninitialized_callback(x); }
int scoped_shadow(int (*callback)(int), int x) {
  if (x) { int (*function)(int) = callback; function(x); }
  return function(x);
}
int parameter_restore(int (*callback)(int), int x) {
  if (x) { int callback = 1; x = callback; }
  return callback(x);
}
int scoped_prototype(int x) {
  if (x) { extern int scoped_external(int); scoped_external(x); }
  return scoped_external(x);
}
int for_scope(int (*callback)(int), int x) {
  for (int (*function)(int) = callback; x > 0; x--) { function(x); }
  return function(x);
}
