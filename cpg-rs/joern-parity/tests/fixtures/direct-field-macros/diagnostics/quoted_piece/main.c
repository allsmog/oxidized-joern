struct Item { int len; int width; int values[4]; };
#define Len len + sizeof("Len")
int f(struct Item *p) { return p->Len; }
