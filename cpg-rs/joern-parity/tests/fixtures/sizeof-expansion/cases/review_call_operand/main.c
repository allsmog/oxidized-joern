#define S(x) sizeof (calc(x))
int calc(int value);
int read(int value) { return S(value); }
