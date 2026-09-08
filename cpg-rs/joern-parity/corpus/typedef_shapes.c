typedef int FunctionType(int);
typedef int ScalarFirst, (*CallbackSecond)(int);
typedef int (*CallbackFirst)(int), ScalarSecond;
typedef int One, Two;
int typedef_shapes_marker(int value) { return value; }
