int produce(void);
void consume(int value);
void z_first(void) {
    int values[2] = {
        produce(),
        2
    };
    consume(values[0]);
}
void a_second(void) {
    int values[2] = {
        produce(),
        2
    };
    consume(values[0]);
}
