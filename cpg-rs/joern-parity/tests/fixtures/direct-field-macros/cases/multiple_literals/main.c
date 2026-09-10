struct Item { int values[4]; int len; };
#define Len len + 1 + 2
int read(struct Item *p) { return p->Len; }
