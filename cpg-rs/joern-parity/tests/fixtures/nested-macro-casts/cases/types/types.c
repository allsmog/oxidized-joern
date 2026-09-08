#define cast(t,e) ((t)(e))
#define U(o) cast(union Thing *, (o))
#define S(o) cast(struct Thing *, (o))
#define E(o) cast(enum Thing *, (o))
#define P(o) cast(unsigned long *, (o))
#define Q(o) cast(long unsigned *, (o))
#define I(o) cast(unsigned int *, (o))
#define C(o) cast(const unsigned long *, (o))
#define V(o) cast(void*, (o))
#define A(o) cast(Alias *, (o))
typedef int Alias;
void *u(void *o){return U(o);}
void *s(void *o){return S(o);}
void *e(void *o){return E(o);}
void *p(void *o){return P(o);}
void *q(void *o){return Q(o);}
void *i(void *o){return I(o);}
void *c(void *o){return C(o);}
void *v(void *o){return V(o);}
void *a(void *o){return A(o);}
