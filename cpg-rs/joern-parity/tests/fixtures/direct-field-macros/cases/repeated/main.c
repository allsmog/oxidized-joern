struct Item { int values[2]; };
#define Len values[1]
int read(struct Item *p) { p->Len = 2; return p->Len; }
