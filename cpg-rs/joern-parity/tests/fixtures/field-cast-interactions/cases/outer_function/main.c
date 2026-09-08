struct Item { int values[4]; int len; };
#define cast(t,e) ((t)(e))
#define Field values[(int)(long)cast(union U *, q)]
#define READ(p) ((p)->Field)
int read(struct Item *p, void *q) { return READ(p); }
