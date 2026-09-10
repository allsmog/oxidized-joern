#define ID(x) x
#define S(x) sizeof (((ID(x))))
int read(int value) { return S(value); }
