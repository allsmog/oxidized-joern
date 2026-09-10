#define RET int
RET *f(const RET *p);
RET *g(const RET *p){return f(p);}
