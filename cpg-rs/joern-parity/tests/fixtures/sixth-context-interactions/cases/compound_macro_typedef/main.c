#define DO(x) { typedef int T; sink((T)(x)); }
void sink(int value);
int f(int x) { DO(x); return x; }
