"""Incremental speech segments at sentence and long-clause boundaries."""

import re


class SentenceBuffer:
    """Retain incomplete text; release sentences or substantial spoken clauses."""

    MIN_CLAUSE_WORDS = 12
    FIRST_CLAUSE_WORDS = 5
    FIRST_MAX_WORDS = 10

    ABBREVIATIONS = {'mr', 'mrs', 'ms', 'dr', 'prof', 'sr', 'jr', 'st', 'vs',
                     'e.g', 'i.e', 'etc', 'approx', 'no', 'fig', 'a.m', 'p.m'}

    def __init__(self):
        self.pending = ''
        self.first_segment = True

    def feed(self, text, final=False):
        self.pending += text
        result = []
        while (end := self._next_boundary(final)) is not None:
            segment = self.pending[:end].strip()
            self.pending = self.pending[end:]
            if segment:
                result.append(segment)
                self.first_segment = False
        if final:
            if self.pending.strip():
                result.append(self.pending.strip())
                self.first_segment = False
            self.pending = ''
        return result

    def _next_boundary(self, final):
        """Pick the earliest usable boundary, independently of stream chunk size."""
        first_limit = None
        if self.first_segment:
            for count, word in enumerate(re.finditer(r'\S+', self.pending), start=1):
                if count == self.FIRST_MAX_WORDS:
                    # A word at the chunk edge might still be arriving.
                    if word.end() < len(self.pending) or final:
                        first_limit = word.end()
                    break

        for match in re.finditer(r'(?:[.!?]+|[,;:\u2014\u2013])["\'\u2019\u201d)\]]*', self.pending):
            end = match.end()
            if first_limit is not None and end > first_limit:
                break
            # Wait for lookahead: a chunk may end inside a decimal or abbreviation.
            if end == len(self.pending) and not final:
                continue
            if end < len(self.pending) and not self.pending[end].isspace():
                continue
            prefix = self.pending[:match.start()]
            if match.group()[0] in ',;:\u2014\u2013':
                # Only split substantial clauses, preserving short lists and pauses.
                # Whitespace lookahead above also protects numbers, times and URLs.
                minimum = self.FIRST_CLAUSE_WORDS if self.first_segment else self.MIN_CLAUSE_WORDS
                if len(prefix.split()) < minimum:
                    continue
            if match.group().startswith('.'):
                word = re.search(r'([\w.]+)$', prefix)
                token = word.group(1).lower() if word else ''
                if token in self.ABBREVIATIONS or re.fullmatch(r'(?:[a-z]\.)*[a-z]', token):
                    continue
                # Numbered list markers are not sentences.
                if token.isdigit() and prefix.strip() == token:
                    continue
            return end
        return first_limit

    def finish(self):
        return self.feed('', final=True)
