use Duallity;

module DictionaryFixture {
    use Libdictenstein;
    our sub create(--> Mu:D) {
        my $dictionary = dynamic-dawg;
        $dictionary.insert-batch([
            ("candidate-$_" => Nil) for 1..10_000
        ]);
        $dictionary
    }
}

my $dictionary = DictionaryFixture::create;
my $view = $dictionary.snapshot;
my $iterations = (%*ENV<DUALLITY_BENCH_ITERATIONS> // 1000).Int;
my $started = now;
for ^$iterations {
    my $graph = wfst($view, 'candidate-5000', maximum-distance => 2);
    $graph.start;
    $graph.close;
}
my $elapsed = now - $started;
say "iterations=$iterations total_seconds=$elapsed ns_per_adapter=" ~
    ($elapsed * 1e9 / $iterations);
$view.close;
$dictionary.close;
