int g = 5;

int first(int a) {
    return a + g;
}

struct pair {
    int *ptr;
    int arr[4];
};

int second(struct pair pr) {
    return pr.arr[0] + first(1);
}
