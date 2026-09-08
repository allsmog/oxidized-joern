#define cast(t,x) ((t)(x))
void *typed(void *p) {return cast(union U *, p); }
