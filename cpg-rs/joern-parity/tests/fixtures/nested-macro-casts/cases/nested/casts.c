#define cast(t,e) ((t)(e))
#define cast_u(o) cast(union GCUnion *, (o))
#define cast_p(o) cast(unsigned long *, (o))
#define rawtt(o) ((o)->tt_)
#define novariant(t) ((t) & 0x0F)
#define ttype(o) novariant(rawtt(o))
#define checktype(o,t) (ttype(o) == (t))
void *union_cast(void *o) { return cast_u(o); }
void *primitive_cast(void *o) { return cast_p(o); }
int tag_test(void *o) { return checktype(o, 3); }
