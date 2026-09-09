int consume(int value);
int probe(void) {
    int included = consume(7);
    consume(7);
    return included;
}
