int consume(const char *a,int b);
#define CALL(a,b) consume(a,b)
int quoted(void) { return CALL("comma,) /* // \\\"", ')'); }
