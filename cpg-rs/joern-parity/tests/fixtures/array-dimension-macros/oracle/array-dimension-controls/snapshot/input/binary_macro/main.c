#define N 3
int macro_dimension(int index) {
    int values[N+1];
    values[0] = 7;
    return values[index];
}
int literal_dimension(int index) {
    int values[3+1];
    values[0] = 7;
    return values[index];
}
