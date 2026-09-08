#define API extern
API int (api_named)(int x);
extern int (plain_named)(int x);
int parentheses_caller(int x) { return api_named(x) + plain_named(x); }
