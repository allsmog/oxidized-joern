struct Pair { int len; int code; };
struct Item { struct Pair dl; struct Pair fc; };
#define Len dl.len
int read(struct Item *p) { return p->Len; }
