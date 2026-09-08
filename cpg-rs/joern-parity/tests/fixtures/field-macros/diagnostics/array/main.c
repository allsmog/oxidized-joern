struct Pair { int len; int code; };
struct Item { struct Pair dl; struct Pair fc; };
#define Len dl.len
#define Code fc.code
#define READ(p, i) ((p)[i].Len + (p)[i].Code)
int read(struct Item *p, int i) { return READ(p, i); }
