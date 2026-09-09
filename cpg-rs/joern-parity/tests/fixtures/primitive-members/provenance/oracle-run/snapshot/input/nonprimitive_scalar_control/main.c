struct Inner {
    int item;
};
union Choice {
    int integer_value;
    double floating_value;
};
typedef int Alias;
struct Record {
    struct Inner struct_value;
    union Choice union_value;
    Alias alias_value;
};

int ordinary(void) {
    return 7;
}
