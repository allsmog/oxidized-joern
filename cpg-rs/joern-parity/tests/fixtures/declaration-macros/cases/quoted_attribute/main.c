#define TYPE int
#define API __attribute__((section("TYPE")))
API TYPE f(TYPE x){return x;}
