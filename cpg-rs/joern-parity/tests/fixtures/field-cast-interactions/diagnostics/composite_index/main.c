struct Item { int values[4]; int len; };
#define Index values[sizeof((int (*)[sizeof((union U *)q)])q)]
int read(struct Item *p, void *q) { return p->Index; }
