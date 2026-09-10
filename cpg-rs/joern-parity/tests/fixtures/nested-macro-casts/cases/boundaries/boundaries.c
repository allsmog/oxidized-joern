#define cast(t,e) ((t)(e))
#define CAST(o) cast(union Thing *, (o))
#define ID(o) (o)
#define SAME(o) SAME(o)
#define TEXT(o) "cast(union Thing *, o)"
void *cast_ordinary(void *o);
void *ordinary(void *o) { return cast_ordinary(o); }
void *nested(void *o) { return CAST(ID(o)); }
void *conditional(void *o, int n) { return CAST(n ? o : 0); }
void *recursive(void *o) { return ID(SAME(o)); }
char *literal(void) { return TEXT(0); }
