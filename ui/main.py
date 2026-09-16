"""Connect the animated avatar overlay to Agent, microphone, and local voice."""

import sys
from pathlib import Path

from PySide6.QtWidgets import QApplication

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from ai.voice.sentences import SentenceBuffer
from agent_chat import AgentWorker, TextChatDialog
from audio_player import VoiceTest
from avatar_assets import AnimationCatalog
from avatar_window import AvatarWindow, prefers_reduced_motion
from microphone import MicrophoneController
from speech_bubble import SpeechBubble


def main():
    app = QApplication(sys.argv)
    app.setApplicationName('MapleSyrup')
    app.setQuitOnLastWindowClosed(False)
    reduced_motion = prefers_reduced_motion()
    avatar = AvatarWindow(AnimationCatalog(Path(__file__).parent / 'animations'), reduced_motion)
    bubble = SpeechBubble()
    worker = AgentWorker(app)
    dialog = TextChatDialog(worker, avatar)
    voice = VoiceTest(app)
    microphone = MicrophoneController(app)
    voice.reduced_motion = reduced_motion
    closing = False
    finished_workers = set()
    sentence_buffer = SentenceBuffer()
    speech_request = None
    speech_blocked = True
    captions_enabled = False
    current_caption = ''

    def follow_bubble(*args):
        screen = avatar.screen() or app.primaryScreen()
        if screen is not None:
            bubble.follow_avatar(avatar.geometry(), screen.availableGeometry())

    def hide_bubble():
        bubble.set_caption('', speaking=False, enabled=False)

    def resting_state():
        return 'listening' if microphone.enabled and microphone.capturing else 'idle'

    def cancel_speech():
        nonlocal speech_blocked
        speech_blocked = True
        voice.stop()
        hide_bubble()

    def resume_listening():
        if microphone.enabled and not microphone.capturing and not closing:
            microphone.start()
        elif not microphone.enabled:
            avatar.set_state('idle')

    def stop_microphone():
        microphone.stop()
        if not closing:
            avatar.set_state('idle')

    def show_text():
        cancel_speech()
        stop_microphone()
        area = (avatar.screen() or app.primaryScreen()).availableGeometry()
        dialog.move(max(area.left(), min(avatar.x() - dialog.width() - 12,
                                         area.right() - dialog.width() + 1)),
                    max(area.top(), min(avatar.y(), area.bottom() - dialog.height() + 1)))
        dialog.show()
        dialog.raise_()
        dialog.activateWindow()

    def show_demo(state):
        cancel_speech()
        stop_microphone()
        avatar.set_state(state)

    def response_started(request_id):
        nonlocal sentence_buffer, speech_request, speech_blocked
        cancel_speech()
        if microphone.capturing:
            microphone.pause()
        sentence_buffer = SentenceBuffer()
        speech_request = request_id
        speech_blocked = not dialog.read_aloud.isChecked()
        avatar.set_state('thinking')
        if not speech_blocked:
            voice.begin()

    def response_delta(request_id, text):
        if request_id != speech_request or closing:
            return
        for sentence in sentence_buffer.feed(text):
            if not speech_blocked:
                voice.enqueue(sentence)

    def response_finished(request_id):
        if request_id != speech_request or closing:
            return
        for sentence in sentence_buffer.finish():
            if not speech_blocked:
                voice.enqueue(sentence)
        if not speech_blocked:
            voice.finish_input()
        else:
            resume_listening()

    def response_failed(request_id):
        if request_id == speech_request:
            cancel_speech()
            resume_listening()

    def present_text(state, text):
        if state == 'Thinking':
            avatar.set_state('thinking')
        elif state in ('Ready', 'Error'):
            avatar.set_state(resting_state())

    def present_caption(previous, current):
        nonlocal current_caption
        current_caption = current

    def present_voice(state, levels, text, pulse):
        nonlocal speech_blocked
        if closing:
            return
        if state == 'Speaking':
            avatar.set_state('speaking')
            bubble.set_caption(current_caption or text, speaking=True, enabled=captions_enabled)
        elif state == 'Thinking':
            avatar.set_state('thinking')
            hide_bubble()
        elif state == 'Error':
            speech_blocked = True
            hide_bubble()
            resume_listening()
        elif state == 'Ready':
            hide_bubble()
            resume_listening()
            avatar.set_state(resting_state())

    def test_voice():
        if closing:
            return
        cancel_speech()
        stop_microphone()
        avatar.set_state('thinking')
        voice.start()

    def stop_voice():
        cancel_speech()
        avatar.set_state(resting_state())

    def read_aloud_changed(enabled):
        nonlocal speech_blocked
        if not enabled:
            speech_blocked = True
            stop_voice()

    def toggle_microphone():
        if microphone.enabled:
            stop_microphone()
            return
        cancel_speech()
        microphone.start()

    def present_microphone(state, text):
        if closing:
            return
        if state == 'Listening':
            avatar.set_state('listening')
        elif state in ('Muted', 'Error'):
            avatar.set_state('idle')

    def submit_recognized_text(token, text):
        if closing or not microphone.capturing or token != microphone.token:
            return
        microphone.pause()
        avatar.set_state('thinking')
        if dialog.submit_player_message(text) is None:
            resume_listening()

    def update_voice_actions():
        avatar.voice_test_action.setEnabled(not closing)
        avatar.stop_voice_action.setEnabled(voice.active and not closing)

    def motion_changed(enabled):
        voice.reduced_motion = enabled
        avatar.set_reduced_motion(enabled)
        voice.update_frame()

    def captions_changed(enabled):
        nonlocal captions_enabled
        captions_enabled = enabled
        bubble.set_caption(current_caption, speaking=avatar.state == 'speaking', enabled=enabled)

    def quit_when_finished(name):
        finished_workers.add(name)
        if closing and len(finished_workers) == 3:
            app.quit()

    def close_all():
        nonlocal closing
        if closing:
            return
        closing = True
        hide_bubble()
        bubble.hide()
        dialog.close()
        avatar.menu.close()
        avatar.hide()
        worker.stop()
        voice.shutdown()
        microphone.shutdown()

    def can_close():
        if closing:
            return len(finished_workers) == 3
        close_all()
        return False

    avatar.can_close = can_close
    avatar.settings_requested.connect(show_text)
    avatar.test_voice_requested.connect(test_voice)
    avatar.stop_voice_requested.connect(stop_voice)
    avatar.motion_requested.connect(motion_changed)
    avatar.captions_toggled.connect(captions_changed)
    avatar.voice_muted.connect(voice.set_muted)
    avatar.exit_requested.connect(close_all)
    avatar.demo_state_requested.connect(show_demo)
    avatar.microphone_toggled.connect(toggle_microphone)
    avatar.moved.connect(follow_bubble)
    voice.frame.connect(present_voice)
    voice.captions.connect(present_caption)
    voice.speech_level.connect(avatar.set_speech_level)
    voice.availability_changed.connect(update_voice_actions)
    dialog.presentation.connect(present_text)
    dialog.response_started.connect(response_started)
    dialog.response_delta.connect(response_delta)
    dialog.response_finished.connect(response_finished)
    dialog.response_failed.connect(response_failed)
    dialog.read_aloud.toggled.connect(read_aloud_changed)
    microphone.state_changed.connect(present_microphone)
    microphone.recognized.connect(submit_recognized_text)
    avatar.closed.connect(close_all)
    worker.finished.connect(lambda: quit_when_finished('agent'))
    voice.worker.finished.connect(lambda: quit_when_finished('voice'))
    microphone.worker.finished.connect(lambda: quit_when_finished('microphone'))
    worker.start()
    area = app.primaryScreen().availableGeometry()
    avatar.move(area.right() - avatar.width() - 16, area.top() + 16)
    avatar.show()
    follow_bubble()
    result = app.exec()
    worker.stop()
    voice.shutdown()
    microphone.shutdown()
    worker.wait()
    voice.worker.wait()
    microphone.worker.wait()
    return result


if __name__ == '__main__':
    sys.exit(main())
