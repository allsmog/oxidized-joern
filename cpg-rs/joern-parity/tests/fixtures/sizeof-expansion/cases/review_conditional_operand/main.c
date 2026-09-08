#define S(x,y) sizeof (x ? y : 0)
int read(int first, int second) { return S(first,second); }
