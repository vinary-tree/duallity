unit module Duallity;

use NativeCall;
need Duallity::GeneratedAbi;
need Duallity::ConfigAbi;
need Vinary::Tree::Interop;

our constant ABI-VERSION is export = Duallity::GeneratedAbi::ABI-VERSION;
our constant API-REVISION is export = Duallity::GeneratedAbi::API-REVISION;
our constant Status is export = Duallity::GeneratedAbi::Status;
our constant OK is export = Duallity::GeneratedAbi::OK;
our constant INVALID-ARGUMENT is export = Duallity::GeneratedAbi::INVALID-ARGUMENT;
our constant INVALID-UTF8 is export = Duallity::GeneratedAbi::INVALID-UTF8;
our constant NULL-POINTER is export = Duallity::GeneratedAbi::NULL-POINTER;
our constant PANIC is export = Duallity::GeneratedAbi::PANIC;
our constant INCOMPATIBLE-RESOURCE is export =
    Duallity::GeneratedAbi::INCOMPATIBLE-RESOURCE;
our constant PROVIDER-ERROR is export = Duallity::GeneratedAbi::PROVIDER-ERROR;
our constant LIMIT-EXCEEDED is export = Duallity::GeneratedAbi::LIMIT-EXCEEDED;

our constant Algorithm is export = Duallity::GeneratedAbi::Algorithm;
our constant STANDARD is export = Duallity::GeneratedAbi::STANDARD;
our constant TRANSPOSITION is export = Duallity::GeneratedAbi::TRANSPOSITION;
our constant MERGE-AND-SPLIT is export = Duallity::GeneratedAbi::MERGE-AND-SPLIT;
our constant DAMERAU-LEVENSHTEIN is export =
    Duallity::GeneratedAbi::DAMERAU-LEVENSHTEIN;

our constant WfstKind is export = Duallity::GeneratedAbi::WfstKind;
our constant LEVENSHTEIN is export = Duallity::GeneratedAbi::LEVENSHTEIN;
our constant UNIVERSAL-STANDARD is export =
    Duallity::GeneratedAbi::UNIVERSAL-STANDARD;
our constant UNIVERSAL-TRANSPOSITION is export =
    Duallity::GeneratedAbi::UNIVERSAL-TRANSPOSITION;
our constant UNIVERSAL-MERGE-AND-SPLIT is export =
    Duallity::GeneratedAbi::UNIVERSAL-MERGE-AND-SPLIT;
our constant GENERALIZED-STANDARD is export =
    Duallity::GeneratedAbi::GENERALIZED-STANDARD;
our constant GENERALIZED-TRANSPOSITION is export =
    Duallity::GeneratedAbi::GENERALIZED-TRANSPOSITION;
our constant GENERALIZED-MERGE-AND-SPLIT is export =
    Duallity::GeneratedAbi::GENERALIZED-MERGE-AND-SPLIT;
our constant GENERALIZED-PHONETIC is export =
    Duallity::GeneratedAbi::GENERALIZED-PHONETIC;
our constant FZF is export = Duallity::GeneratedAbi::FZF;
our constant CachePolicy is export = Duallity::ConfigAbi::CachePolicy;
our constant CACHE-ALL is export = Duallity::ConfigAbi::CACHE-ALL;
our constant NO-CACHE is export = Duallity::ConfigAbi::NO-CACHE;
our constant LRU is export = Duallity::ConfigAbi::LRU;
our constant OperationApplicability is export =
    Duallity::ConfigAbi::OperationApplicability;

module InteropAccess {
    use Vinary::Tree::Interop;

    our constant ResourceType = Resource;
    our constant DictionaryType = Dictionary;
    our constant WfstType = Wfst;
    our constant RawResourceType = RawResource;

    our sub adopt(RawResource:D $raw --> Resource:D) { adopt-resource($raw) }
    our sub wrap(Resource:D $resource --> Wfst:D) { wfst($resource, :take) }
}

class X::Duallity is Exception {
    has Status:D $.status is required;
    has Str:D $.operation is required;
    has Str:D $.detail = '';

    method message(--> Str:D) {
        my $base = "duallity operation '$!operation' failed with $!status";
        $!detail.chars ?? "$base: $!detail" !! $base
    }
}

sub abi-version(--> UInt:D) is export {
    Duallity::GeneratedAbi::duallity-abi-version().UInt
}
sub api-revision(--> UInt:D) is export {
    Duallity::GeneratedAbi::duallity-api-revision().UInt
}

sub check-status(Int:D $code, Str:D $operation --> Nil) {
    my $status = Status($code);
    return if $status == OK;
    X::Duallity.new(
        :$status,
        :$operation,
        detail =>
            (try Duallity::GeneratedAbi::duallity-last-error-message()) // '',
    ).throw;
}

multi sub raw-resource(InteropAccess::ResourceType:D $resource
    --> InteropAccess::RawResourceType:D) {
    $resource.raw
}
multi sub raw-resource(InteropAccess::DictionaryType:D $dictionary
    --> InteropAccess::RawResourceType:D) {
    $dictionary.resource.raw
}

sub adopt-wfst(Pointer:D $handle --> InteropAccess::WfstType:D) {
    my $raw = InteropAccess::RawResourceType.new;
    check-status(
        Duallity::GeneratedAbi::duallity-wfst-resource($handle, $raw),
        'wfst-resource',
    );
    InteropAccess::wrap(InteropAccess::adopt($raw))
}

sub wfst(
    Mu:D $dictionary,
    Str:D $query,
    UInt:D :$maximum-distance = 1,
    Algorithm:D :$algorithm = STANDARD,
    WfstKind:D :$kind = LEVENSHTEIN,
    --> InteropAccess::WfstType:D
) is export {
    my $bytes = $query.encode('utf8');
    my Pointer $output .= new;
    my $data = $bytes.elems ?? nativecast(Pointer, $bytes) !! Pointer;
    check-status(
        Duallity::GeneratedAbi::duallity-wfst-new-ref(
            raw-resource($dictionary),
            $data,
            $bytes.elems,
            $maximum-distance,
            $algorithm,
            $kind,
            $output,
        ),
        'wfst-new',
    );
    LEAVE Duallity::GeneratedAbi::duallity-wfst-free($output);
    adopt-wfst($output)
}

sub copy-config-text(Int:D $data, UInt:D $length --> Str:D) {
    return '' unless $length;
    die 'native config text exceeds ABI maximum'
        if $length > Duallity::ConfigAbi::CONFIG-MAX-CUSTOM-TEXT-BYTES;
    die 'native returned null config text' unless $data;
    my $bytes = nativecast(CArray[uint8], Pointer.new($data));
    Buf.new((^$length).map({ $bytes[$_] })).decode('utf8')
}

sub config-record(
    Mu:U $type,
    --> Mu:D
) {
    $type.new(
        struct-size => nativesizeof($type),
        record-version => Duallity::ConfigAbi::CONFIG-RECORD-VERSION,
    )
}

sub default-options(--> Duallity::ConfigAbi::DuallityWfstOptionsV1:D)
    is export {
    my $record = config-record(Duallity::ConfigAbi::DuallityWfstOptionsV1);
    check-status(
        Duallity::GeneratedAbi::duallity-wfst-options-default(
            nativecast(Pointer, $record)),
        'wfst-options-default',
    );
    $record
}

class ConfiguredWfst is export {
    has Mu:D $.graph is required;
    has Pointer $!handle;

    submethod BUILD(:$!graph, :$!handle) { }

    method !live-handle(--> Pointer:D) {
        die 'configured WFST is closed' unless $!handle.defined;
        $!handle
    }

    method close(--> ConfiguredWfst:D) {
        return self unless $!handle.defined;
        my $handle = $!handle;
        $!handle = Pointer;
        LEAVE Duallity::GeneratedAbi::duallity-wfst-free($handle);
        $!graph.close;
        self
    }

    method DESTROY() { self.close if $!handle.defined }

    method options(--> Map:D) {
        my $raw = config-record(Duallity::ConfigAbi::DuallityWfstOptionsV1);
        check-status(
            Duallity::GeneratedAbi::duallity-wfst-options-get(
                self!live-handle, nativecast(Pointer, $raw)),
            'wfst-options-get',
        );
        my $count = $raw.operation-count.Int;
        die 'native operation count exceeds ABI maximum'
            if $count > Duallity::ConfigAbi::CONFIG-MAX-OPERATIONS;
        die 'native returned null operations' if $count && !$raw.operations;
        die 'native operation stride is too small'
            if $count && $raw.operation-stride <
                nativesizeof(Duallity::ConfigAbi::DuallityOperationV1);
        my @operations;
        for ^$count -> $index {
            my $item = nativecast(Duallity::ConfigAbi::DuallityOperationV1,
                Pointer.new($raw.operations +
                    $index * $raw.operation-stride));
            my $pair-count = $item.restriction-count.Int;
            die 'native restriction count exceeds ABI maximum'
                if $pair-count > Duallity::ConfigAbi::CONFIG-MAX-RESTRICTION-PAIRS;
            die 'native returned null restrictions'
                if $pair-count && !$item.restrictions;
            die 'native restriction stride is too small'
                if $pair-count && $item.restriction-stride <
                    nativesizeof(Duallity::ConfigAbi::DuallityRestrictionV1);
            my @restrictions;
            for ^$pair-count -> $pair-index {
                my $pair = nativecast(Duallity::ConfigAbi::DuallityRestrictionV1,
                    Pointer.new($item.restrictions +
                        $pair-index * $item.restriction-stride));
                @restrictions.push(Map.new(
                    source => copy-config-text($pair.source-data,
                        $pair.source-len.UInt),
                    target => copy-config-text($pair.target-data,
                        $pair.target-len.UInt),
                ));
            }
            @operations.push(Map.new(
                consume-x => $item.consume-x,
                consume-y => $item.consume-y,
                weight => $item.weight,
                applicability => OperationApplicability($item.applicability),
                name => copy-config-text($item.name-data, $item.name-len.UInt),
                restrictions => @restrictions.List,
            ));
        }
        my $limits = $raw.limits
            ?? nativecast(Duallity::ConfigAbi::DuallityGeneralizedLimitsV1,
                Pointer.new($raw.limits))
            !! Nil;
        Map.new(
            kind => WfstKind($raw.kind),
            algorithm => Algorithm($raw.algorithm),
            maximum-distance => $raw.maximum-distance,
            cache-policy => CachePolicy($raw.cache-policy),
            cache-capacity => $raw.cache-capacity,
            limits => $limits.defined ?? Map.new(
                max-query-bytes => $limits.max-query-bytes,
                max-query-scalars => $limits.max-query-scalars,
                max-operation-source-scalars =>
                    $limits.max-operation-source-scalars,
                max-operation-query-scalars =>
                    $limits.max-operation-query-scalars,
                max-retained-dictionary-nodes =>
                    $limits.max-retained-dictionary-nodes,
                max-retained-wfst-states => $limits.max-retained-wfst-states,
                max-paths-per-expansion => $limits.max-paths-per-expansion,
                max-work-units-per-expansion =>
                    $limits.max-work-units-per-expansion,
            ) !! Nil,
            operations => @operations.List,
        )
    }

    method cache-statistics(--> Duallity::ConfigAbi::DuallityCacheStatisticsV1:D) {
        my $record = config-record(Duallity::ConfigAbi::DuallityCacheStatisticsV1);
        check-status(
            Duallity::GeneratedAbi::duallity-wfst-cache-statistics(
                self!live-handle, nativecast(Pointer, $record)),
            'wfst-cache-statistics',
        );
        $record
    }

    method clear-cache(--> ConfiguredWfst:D) {
        check-status(
            Duallity::GeneratedAbi::duallity-wfst-cache-clear(
                self!live-handle),
            'wfst-cache-clear',
        );
        self
    }

    method set-cache-policy(
        Duallity::ConfigAbi::CachePolicy:D $policy,
        UInt:D :$capacity = 0,
        --> ConfiguredWfst:D
    ) {
        die 'cache capacity exceeds uint64_t' if $capacity > 2**64 - 1;
        check-status(
            Duallity::GeneratedAbi::duallity-wfst-cache-set-policy(
                self!live-handle, $policy, $capacity),
            'wfst-cache-set-policy',
        );
        self
    }
}

sub configured-wfst(
    Mu:D $dictionary,
    Str:D $query,
    Duallity::ConfigAbi::DuallityWfstOptionsV1:D $options,
    :$keepalive,
    --> ConfiguredWfst:D
) is export {
    die 'nested raw pointers require keepalive buffers'
        if ($options.limits || $options.operations) &&
            !$keepalive.defined;
    my $bytes = $query.encode('utf8');
    my $data = $bytes.elems ?? nativecast(Pointer, $bytes) !! Pointer;
    my Pointer $output .= new;
    my $transferred = False;
    LEAVE Duallity::GeneratedAbi::duallity-wfst-free($output)
        if $output.defined && !$transferred;
    my $pin = [$options, $bytes, $keepalive];
    LEAVE $pin.elems;
    check-status(
        Duallity::GeneratedAbi::duallity-wfst-new-configured-ref(
            raw-resource($dictionary), $data, $bytes.elems,
            nativecast(Pointer, $options), $output),
        'wfst-new-configured',
    );
    my $graph = adopt-wfst($output);
    $transferred = True;
    ConfiguredWfst.new(:$graph, handle => $output)
}

INIT {
    die "duallity ABI mismatch: native {abi-version()} / facade {ABI-VERSION}"
        unless abi-version() == ABI-VERSION;
    die "duallity API revision {api-revision()} is older than {API-REVISION}"
        unless api-revision() >= API-REVISION;
}
