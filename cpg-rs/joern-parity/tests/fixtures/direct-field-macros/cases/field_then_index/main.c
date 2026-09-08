struct Item { int values[4]; int len; };
#define Values values
int read(struct Item *p, int index) { return p->Values[index]; }
