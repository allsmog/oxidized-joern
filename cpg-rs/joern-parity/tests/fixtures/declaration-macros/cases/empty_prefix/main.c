#define API extern
API int f(int x);
int g(int x) {return f(x);}
