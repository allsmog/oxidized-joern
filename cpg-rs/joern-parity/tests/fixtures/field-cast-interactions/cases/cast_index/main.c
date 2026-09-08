struct Item { int values[4]; int len; };
#define Index values[(int)(long)((union U *)q)]
int read(struct Item *p, void *q) { return p->Index; }
