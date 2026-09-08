#define API __attribute__((visibility("internal"))) extern
API int f(int x);
int g(int x){return f(x);}
