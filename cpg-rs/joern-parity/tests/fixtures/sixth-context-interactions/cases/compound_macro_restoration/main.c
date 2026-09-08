#define DO(x) { typedef int T; sink((T)(x)); }
#define C(x) ((T)(x))
void sink(int value);
int f(int x) { DO(x); return C(x); }
