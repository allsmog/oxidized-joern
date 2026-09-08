struct Pair { int len; int code; };
struct Item { struct Pair dl; struct Pair fc; };
#define Len dl.len
int read(struct Item item) { return item.Len; }
