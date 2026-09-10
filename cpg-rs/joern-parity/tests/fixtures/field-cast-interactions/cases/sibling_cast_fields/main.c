struct Item { int values[4]; int len; };
#define Left values[(int)(long)((union U *)q)]
#define Right values[(int)(long)((struct V *)q)]
int read(struct Item *p, void *q) { return p->Left + p->Right; }
