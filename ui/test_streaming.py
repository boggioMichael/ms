"""Streaming contracts, sentence boundaries, completed memory, and queue timing."""

import io
import json
from pathlib import Path
import sys
from types import SimpleNamespace
import unittest
from unittest.mock import Mock, patch

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ai.agent import Agent
from ai.llm.ollama_provider import OllamaProvider
from ai.llm.openai_provider import OpenAIProvider
from ai.voice.sentences import SentenceBuffer
import test_audio_player as audio_tests

FakeTTS = audio_tests.FakeTTS
from PySide6.QtMultimedia import QMediaPlayer
from PySide6.QtTest import QTest
import avatar_window


LONG_REPLY_SEGMENTS = [
    "I'm a text-based assistant trained to help with general MapleStory",
    "questions and knowledge, but I don't have access to the player's in-game data,",
    "personal preferences, or current game state, and my knowledge may be outdated,",
    "so I can only provide information based on our conversation and my training data.",
]
LONG_REPLY = ' '.join(LONG_REPLY_SEGMENTS)


class StreamingTests(unittest.TestCase):
    def test_first_segment_prefers_early_natural_pause(self):
        for punctuation in (',', ';', ':', '\u2014', '\u2013', ',"'):
            prefix = 'You can explore the world' + punctuation
            text = prefix + ' and build your own character as you go.'
            for width in (1, 7, 1000):
                with self.subTest(punctuation=punctuation, width=width):
                    buffer = SentenceBuffer()
                    segments = []
                    for i in range(0, len(text), width):
                        segments.extend(buffer.feed(text[i:i + width]))
                    segments.extend(buffer.finish())
                    self.assertEqual(segments, [prefix, 'and build your own character as you go.'])

    def test_tenth_word_waits_until_complete_then_releases_immediately(self):
        buffer = SentenceBuffer()
        first = 'MapleStory lets you explore a colorful world with many characters'
        partial = first.removesuffix('characters') + 'char'
        self.assertEqual(buffer.feed(partial), [])
        self.assertEqual(buffer.feed('acters'), [])
        self.assertEqual(buffer.feed(' '), [first])
        self.assertEqual(buffer.finish(), [])

    def test_only_first_segment_has_a_word_limit(self):
        first = 'MapleStory lets you explore a colorful world with many characters'
        later = 'After that you can keep exploring and meet many other players along the way'
        buffer = SentenceBuffer()
        self.assertEqual(buffer.feed(first + ' '), [first])
        self.assertEqual(buffer.feed(later + ' '), [])
        self.assertEqual(buffer.finish(), [later])
        fresh = SentenceBuffer()
        self.assertEqual(fresh.feed(first + ' '), [first])

    def test_first_word_limit_preserves_numbers_and_short_final_text(self):
        text = 'You should prepare for this adventure by bringing exactly 1,000 coins'
        for width in (1, 4, 1000):
            buffer = SentenceBuffer()
            segments = []
            for i in range(0, len(text), width):
                segments.extend(buffer.feed(text[i:i + width]))
            segments.extend(buffer.finish())
            self.assertEqual(segments, [text.removesuffix(' coins'), 'coins'])
        buffer = SentenceBuffer()
        self.assertEqual(buffer.feed('Hello there'), [])
        self.assertEqual(buffer.finish(), ['Hello there'])

    def test_long_reply_clauses_across_arbitrary_chunks(self):
        for width in (1, 2, 5, 12, 1000):
            with self.subTest(width=width):
                buffer = SentenceBuffer()
                segments = []
                for i in range(0, len(LONG_REPLY), width):
                    segments.extend(buffer.feed(LONG_REPLY[i:i + width]))
                segments.extend(buffer.finish())
                self.assertEqual(segments, LONG_REPLY_SEGMENTS)
                self.assertEqual(' '.join(segments), LONG_REPLY)

    def test_long_clause_before_sentence_end_and_final_tail(self):
        buffer = SentenceBuffer()
        self.assertEqual(buffer.feed(LONG_REPLY_SEGMENTS[0]), [])
        self.assertEqual(buffer.feed(' unfinished continuation'), [LONG_REPLY_SEGMENTS[0]])
        self.assertEqual(buffer.finish(), ['unfinished continuation'])
        self.assertEqual(buffer.finish(), [])

    def test_later_soft_boundaries_preserve_short_clauses_numbers_and_times(self):
        short = 'Buy potions, arrows, and food; then rest: you need it.'
        buffer = SentenceBuffer()
        self.assertEqual(buffer.feed('Welcome. '), ['Welcome.'])
        self.assertEqual(buffer.feed(short + ' '), [short])
        prefix = ('For the next training session you should bring enough potions and '
                  'reserve at least 1,000 coins before 10:30')
        buffer = SentenceBuffer()
        self.assertEqual(buffer.feed('Welcome. '), ['Welcome.'])
        for character in prefix:
            self.assertEqual(buffer.feed(character), [])
        self.assertEqual(buffer.feed(', then rest'), [prefix + ','])
        self.assertEqual(buffer.finish(), ['then rest'])

    def test_other_clause_boundaries_and_closing_quotes(self):
        prefix = 'For the next training session you should bring enough potions and reserve some extra coins'
        for punctuation in (';', ':', '\u2014', '\u2013', ',"'):
            with self.subTest(punctuation=punctuation):
                buffer = SentenceBuffer()
                self.assertEqual(buffer.feed('Welcome. '), ['Welcome.'])
                text = prefix + punctuation + ' then rest'
                segments = []
                for character in text:
                    segments.extend(buffer.feed(character))
                segments.extend(buffer.finish())
                self.assertEqual(segments, [prefix + punctuation, 'then rest'])

    def test_arbitrary_chunks_abbreviations_decimals_and_tail(self):
        text = 'Dr. Smith has 3.14 coins. Try e.g. the shop! "Ready?" Final words'
        expected = ['Dr. Smith has 3.14 coins.', 'Try e.g. the shop!', '"Ready?"', 'Final words']
        for width in (1, 2, 5, 12, 1000):
            buffer = SentenceBuffer()
            sentences = []
            for i in range(0, len(text), width):
                sentences.extend(buffer.feed(text[i:i + width]))
            sentences.extend(buffer.finish())
            self.assertEqual(sentences, expected)

    def test_sentence_before_stream_end(self):
        buffer = SentenceBuffer()
        self.assertEqual(buffer.feed('First sentence.'), [])
        self.assertEqual(buffer.feed(' Next'), ['First sentence.'])
        self.assertEqual(buffer.finish(), ['Next'])

    def test_memory_completion_failure_and_generator_close(self):
        class Provider:
            def stream(self, **kwargs):
                yield 'First. '
                yield 'Second.'
        agent = Agent(Provider())
        stream = agent.respond_stream('hello')
        self.assertEqual(next(stream), 'First. ')
        self.assertEqual(len(agent.memory.turns), 0)
        self.assertEqual(''.join(stream), 'Second.')
        self.assertEqual(list(agent.memory.turns), [{'user':'hello','assistant':'First. Second.'}])
        stream = agent.respond_stream('cancel')
        next(stream)
        stream.close()
        self.assertEqual(len(agent.memory.turns), 1)
        class Broken:
            def stream(self, **kwargs):
                yield 'Partial'
                raise RuntimeError('broken')
        agent.provider = Broken()
        with self.assertRaises(RuntimeError):
            list(agent.respond_stream('failed'))
        self.assertEqual(len(agent.memory.turns), 1)
        canceled = False
        agent.provider = Provider()
        stream = agent.respond_stream('obsolete', lambda: canceled)
        next(stream)
        canceled = True
        self.assertEqual(list(stream), [])
        self.assertEqual(len(agent.memory.turns), 1)

    def test_ollama_real_ndjson_contract_and_incomplete_stream(self):
        content = b'\n'.join(json.dumps(x).encode() for x in [
            {'message':{'thinking':'private reasoning','content':''}, 'done':False},
            {'message':{'content':'Hello '}, 'done':False},
            {'message':{'content':'world.'}, 'done':True},
        ])
        with patch('ai.llm.ollama_provider.urlopen', return_value=io.BytesIO(content)) as request:
            self.assertEqual(list(OllamaProvider('test').stream('rules','hi')), ['Hello ', 'world.'])
            payload = json.loads(request.call_args.args[0].data)
            self.assertTrue(payload['stream'])
            self.assertEqual(payload['messages'][-1], {'role':'user','content':'hi'})
        with patch('ai.llm.ollama_provider.urlopen', return_value=io.BytesIO(b'{"message":{"content":"partial"}}')):
            agent = Agent(OllamaProvider('test'))
            with self.assertRaises(RuntimeError):
                list(agent.respond_stream('hi'))
            self.assertFalse(agent.memory.turns)

    def test_openai_sse_event_contract_and_failure(self):
        with patch('ai.llm.openai_provider.OpenAI') as client:
            provider = OpenAIProvider('test')
            stream = client.return_value.responses.create.return_value
            stream.__enter__.return_value = iter([
                SimpleNamespace(type='response.output_text.delta', delta='Hello.'),
                SimpleNamespace(type='response.completed'),
            ])
            self.assertEqual(list(provider.stream('rules','hi')), ['Hello.'])
            self.assertTrue(client.return_value.responses.create.call_args.kwargs['stream'])
            stream.__exit__.assert_called_once()
            stream.__enter__.return_value = iter([
                SimpleNamespace(type='response.output_text.delta', delta='partial'),
                SimpleNamespace(type='response.incomplete'),
            ])
            agent = Agent(provider)
            with self.assertRaises(RuntimeError):
                list(agent.respond_stream('hi'))
            self.assertFalse(agent.memory.turns)

    def test_motion_default_and_explicit_preference(self):
        with patch.object(avatar_window.sys, 'platform', 'darwin'), patch('avatar_window.subprocess.run') as run:
            run.return_value = SimpleNamespace(returncode=1, stdout='')
            self.assertFalse(avatar_window.prefers_reduced_motion())
            run.return_value = SimpleNamespace(returncode=0, stdout='1\n')
            self.assertTrue(avatar_window.prefers_reduced_motion())
            run.return_value = SimpleNamespace(returncode=0, stdout='0\n')
            self.assertFalse(avatar_window.prefers_reduced_motion())


class SentenceQueueTests(unittest.TestCase):
    setUpClass = classmethod(audio_tests.VoiceTests.setUpClass.__func__)
    setUp = audio_tests.VoiceTests.setUp
    tearDown = audio_tests.VoiceTests.tearDown
    wait_generated = audio_tests.VoiceTests.wait_generated

    def test_clause_playback_starts_before_full_reply_and_preserves_memory(self):
        class Provider:
            def stream(self, **kwargs):
                yield LONG_REPLY_SEGMENTS[0] + ' '
                yield ' '.join(LONG_REPLY_SEGMENTS[1:])

        agent = Agent(Provider())
        stream = agent.respond_stream('What can you help with?')
        buffer = SentenceBuffer()
        self.voice.begin()
        first = next(stream)
        for segment in buffer.feed(first):
            self.voice.enqueue(segment)
        self.wait_generated()
        self.voice.update_frame()
        self.assertEqual(self.voice.current_sentence, LONG_REPLY_SEGMENTS[0])
        self.assertFalse(agent.memory.turns)

        full_response = first
        for delta in stream:
            full_response += delta
            for segment in buffer.feed(delta):
                self.voice.enqueue(segment)
        for segment in buffer.finish():
            self.voice.enqueue(segment)
        self.voice.finish_input()
        for _ in range(200):
            if len(self.voice.ready_audio) == 2:
                break
            QTest.qWait(10)
        self.assertEqual(FakeTTS.requests, LONG_REPLY_SEGMENTS[:3])
        self.assertEqual(self.voice.player.play.call_count, 1)
        self.assertEqual(full_response, LONG_REPLY)
        self.assertEqual(len(agent.memory.turns), 1)
        self.assertEqual(agent.memory.turns[0]['assistant'], LONG_REPLY)
        for index, segment in enumerate(LONG_REPLY_SEGMENTS):
            self.voice.update_frame()
            self.assertEqual(self.voice.current_sentence, segment)
            self.assertEqual(self.voice.previous_sentence, LONG_REPLY_SEGMENTS[index - 1] if index else '')
            self.voice.on_media_status(QMediaPlayer.MediaStatus.EndOfMedia)
            self.wait_generated()
        self.assertEqual(FakeTTS.requests, LONG_REPLY_SEGMENTS)
        self.assertFalse(self.voice.active)

    def test_queue_order_prefetch_no_overlap_and_captions(self):
        captions = []
        self.voice.captions.connect(lambda previous, current: captions.append((previous,current)))
        self.voice.begin()
        self.voice.enqueue('First.')
        self.voice.enqueue('Second.')
        self.voice.enqueue('Third.')
        self.voice.finish_input()
        for _ in range(200):
            if len(self.voice.ready_audio) == 2:
                break
            QTest.qWait(10)
        self.assertEqual(FakeTTS.requests, ['First.', 'Second.', 'Third.'])
        self.assertEqual(self.voice.player.play.call_count, 1)
        self.assertEqual(len(self.voice.ready_audio), 2)
        for index, text in enumerate(('First.', 'Second.', 'Third.')):
            self.voice.update_frame()
            self.assertEqual(self.voice.current_sentence, text)
            self.assertEqual(self.voice.player.play.call_count, index+1)
            self.voice.on_media_status(QMediaPlayer.MediaStatus.EndOfMedia)
        self.assertEqual(captions[-3:], [('', 'First.'), ('First.', 'Second.'), ('Second.', 'Third.')])
        self.assertFalse(self.voice.active)

    def test_stop_clears_prepared_and_not_yet_synthesized_sentences(self):
        self.voice.begin()
        for i in range(8):
            self.voice.enqueue(f'Sentence {i}.')
        self.voice.finish_input()
        for _ in range(100):
            if len(self.voice.ready_audio) == 1:
                break
            QTest.qWait(10)
        self.voice.stop()
        self.assertFalse(self.voice.ready_audio)
        self.assertFalse(self.voice.pending_sentences)
        self.voice.start('Replacement.')
        self.wait_generated()
        self.voice.update_frame()
        self.assertEqual(self.voice.current_sentence, 'Replacement.')


class CaptionRenderTests(unittest.TestCase):
    def test_current_caption_bubble_has_stable_size_and_hides_after_playback(self):
        from PySide6.QtWidgets import QApplication
        from speech_bubble import SpeechBubble
        app = QApplication.instance() or QApplication([])
        bubble = SpeechBubble()
        bubble.set_caption('Keep going!', speaking=True, enabled=True)
        QTest.qWait(180)
        app.processEvents()
        size = bubble.size()
        first = bubble.grab().toImage()
        bubble.set_caption('A different current sentence.', speaking=True, enabled=True)
        app.processEvents()
        self.assertNotEqual(first, bubble.grab().toImage())
        self.assertEqual(bubble.label.text(), 'A different current sentence.')
        self.assertEqual(bubble.size(), size)
        bubble.set_caption('', speaking=False, enabled=True)
        QTest.qWait(180)
        self.assertFalse(bubble.isVisible())
        bubble.close()


if __name__ == '__main__':
    unittest.main()
