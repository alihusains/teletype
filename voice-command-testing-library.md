Voice Command Testing Library
Complete Reference for Voice Input, Intent Routing, and Command Execution

Purpose: Provide a comprehensive command reference for users and a test dataset specification for developers implementing a voice-control pipeline using Parakeet STT, dictionary correction, filler removal, inverse text normalization (ITN), TinyDecide.classify_intent(), Needle 3.extract_command(), and CommandExecutor.

Table of Contents
System Overview
Rules for Users
Application Launching
Brightness and Display Controls
Volume and Audio Controls
System and Window Controls
Browser Commands
Terminal and Developer Commands
File and Folder Management
Keyboard and Text Editing
Clipboard Commands
Dictation and Text Transformation
Search Commands
Multi-Action Commands
Ambiguous Inputs
Negative Tests and False Positives
Speech Recognition Robustness
Unsupported and Incomplete Commands
Safety and Confirmation
Suggested Intent Taxonomy
Structured Test Dataset
Evaluation Metrics
Recommended Testing Checklist
User Quick Reference
1. System Overview

The voice input pipeline has two primary paths.

Dictation path
Capture microphone audio.
Transcribe speech using Parakeet.
Apply dictionary corrections.
Remove fillers when appropriate.
Apply inverse text normalization.
Classify intent using TinyDecide.classify_intent(text).
Protect special text and selections with AutoText.
Transform the selected text if required.
Polish the dictated text using S1-mini.
Restore protected text and emojis.
Inject the final text through the clipboard and paste mechanism.
Command path
Capture and normalize speech.
Classify intent using TinyDecide.classify_intent(text).
Extract the structured command using Needle 3.extract_command(text).
Validate the action name and arguments.
Check permissions, application context, and confirmation requirements.
Execute the action using CommandExecutor.
Report success or failure to the user.
Routing principle
Dictation: The user wants words written, edited, or transformed.
Command: The user wants the computer or an application to perform an action.
Ambiguous: Ask for clarification or default to a safe, non-executing behavior.

A high-confidence intent classification must not automatically authorize dangerous actions.

2. Rules for Users

Users should speak naturally. Exact phrases are examples, not mandatory syntax.

User goal	Example phrase
Launch an application	"Open Brave."
Change a setting	"Make the screen brighter."
Control audio	"Turn the volume down."
Navigate a browser	"Open a new tab."
Work with files	"Open my downloads folder."
Run a development command	"Run npm test."
Edit selected text	"Capitalize the selected text."
Dictate a sentence	"The report is ready for review."
Ask for an explanation	"Explain what this command does."
Perform multiple actions	"Open Brave and go to GitHub."
Helpful command patterns
Open: "Open [application or folder]."
Launch: "Launch [application]."
Set: "Set [setting] to [value]."
Increase: "Increase [setting]."
Decrease: "Lower [setting]."
Mute: "Mute [audio device]."
Search: "Search for [query]."
Navigate: "Go to [website or destination]."
Run: "Run [command]."
Create: "Create [file or folder]."
Rename: "Rename [item] to [new name]."
Select: "Select [text or item]."
Replace: "Replace [old text] with [new text]."
Sequence: "[First action], then [second action]."

Users should specify the target and value when a command could otherwise be interpreted in several ways.

3. Application Launching
3.1 Basic application commands
Spoken input	Expected intent	Suggested action
"Open Brave."	Command	open_app(app="brave")
"Launch Brave."	Command	open_app(app="brave")
"Start Brave."	Command	open_app(app="brave")
"Open Firefox."	Command	open_app(app="firefox")
"Launch Chrome."	Command	open_app(app="chrome")
"Start Safari."	Command	open_app(app="safari")
"Open Visual Studio Code."	Command	open_app(app="vscode")
"Launch VS Code."	Command	open_app(app="vscode")
"Open my terminal."	Command	open_app(app="terminal")
"Launch calculator."	Command	open_app(app="calculator")
"Open settings."	Command	open_app(app="settings")
"Open the file manager."	Command	open_app(app="file_manager")
"Open Slack."	Command	open_app(app="slack")
"Open Discord."	Command	open_app(app="discord")
"Open my email application."	Command	open_app(app="email")
"Open the task manager."	Command	open_app(app="task_manager")
3.2 Application switching
Spoken input	Suggested action
"Switch to Brave."	switch_app(app="brave")
"Switch to VS Code."	switch_app(app="vscode")
"Bring the terminal to the front."	switch_app(app="terminal")
"Return to the browser."	switch_app(app="browser")
"Switch to the previous application."	switch_app(direction="previous")
"Show me the calculator."	switch_app(app="calculator")
3.3 Application aliases

The application resolver should map known aliases to canonical application identifiers.

Spoken variants	Canonical target
"VS Code", "Visual Studio Code", "Code editor"	vscode
"Google Chrome", "Chrome browser"	chrome
"Firefox browser", "Mozilla Firefox"	firefox
"Brave browser", "Brave"	brave
"Command line", "terminal", "console"	terminal, subject to platform configuration
"Calculator app", "calculator"	calculator
"File browser", "file manager"	file_manager

Aliases should only be accepted when they are configured and unambiguous in the current environment.

4. Brightness and Display Controls
Spoken input	Suggested action	Arguments
"Increase the brightness."	set_brightness	direction="increase"
"Make the screen brighter."	set_brightness	direction="increase"
"Turn the brightness up."	set_brightness	direction="increase"
"Lower the brightness."	set_brightness	direction="decrease"
"Dim the screen."	set_brightness	direction="decrease"
"Make the screen darker."	set_brightness	direction="decrease"
"Set brightness to 30 percent."	set_brightness	value=30
"Set brightness to 50 percent."	set_brightness	value=50
"Set brightness to maximum."	set_brightness	value=100
"Set brightness to minimum."	set_brightness	value=0
"Turn on dark mode."	set_theme	theme="dark"
"Turn off dark mode."	set_theme	theme="light"
"Switch to light mode."	set_theme	theme="light"
"Switch to dark mode."	set_theme	theme="dark"
"Take a screenshot."	take_screenshot	No arguments
"Capture the screen."	take_screenshot	No arguments
"Capture the active window."	take_screenshot	target="active_window"
"Show me the desktop."	show_desktop	No arguments
"Minimize all windows."	minimize_all_windows	No arguments
Brightness test considerations
Normalize percentages to numbers.
Reject or clamp values outside the supported range according to product policy.
Distinguish screen brightness from keyboard backlight brightness.
Confirm which display to adjust if multiple monitors are present and the target is unclear.
Do not claim a setting changed until the underlying operation succeeds.
5. Volume and Audio Controls
Spoken input	Suggested action	Arguments
"Turn the volume up."	set_volume	direction="increase"
"Increase the volume."	set_volume	direction="increase"
"Make it louder."	set_volume	direction="increase"
"Turn the volume down."	set_volume	direction="decrease"
"Lower the volume."	set_volume	direction="decrease"
"Make it quieter."	set_volume	direction="decrease"
"Set the volume to 20 percent."	set_volume	value=20
"Set the volume to 50 percent."	set_volume	value=50
"Set the volume to maximum."	set_volume	value=100
"Mute the speaker."	mute_speaker	muted=true
"Mute the speakers."	mute_speaker	muted=true
"Unmute the speaker."	mute_speaker	muted=false
"Turn the sound back on."	mute_speaker	muted=false
"Mute my microphone."	mute_microphone	muted=true
"Unmute my microphone."	mute_microphone	muted=false
"Switch to my headphones."	set_audio_output	device="headphones"
"Use the laptop speakers."	set_audio_output	device="speakers"
"Switch to the USB microphone."	set_audio_input	device="usb_microphone"
Audio test considerations
Speaker mute and microphone mute are different actions.
"Turn it off" is ambiguous unless the active device or setting is known.
"Mute my microphone" should not mute system output.
Device names should be resolved against actual connected devices.
Check the platform's capabilities before exposing an action to users.
6. System and Window Controls
Spoken input	Suggested action
"Lock my computer."	lock_screen()
"Lock the screen."	lock_screen()
"Put my computer to sleep."	sleep_system()
"Show the desktop."	show_desktop()
"Minimize this window."	minimize_window(target="active")
"Maximize this window."	maximize_window(target="active")
"Restore this window."	restore_window(target="active")
"Close this window."	close_window(target="active")
"Switch to the next window."	switch_window(direction="next")
"Switch to the previous window."	switch_window(direction="previous")
"Move this window to the left."	move_window(position="left")
"Move this window to the right."	move_window(position="right")
"Move this window to the other monitor."	move_window(target="other_monitor")
"Open the system settings."	open_app(app="settings")

Shutdown, restart, sleep, and other disruptive operations should be handled according to explicit user-consent and confirmation policies.

7. Browser Commands
7.1 Tabs and navigation
Spoken input	Suggested action
"Open a new tab."	browser_new_tab()
"Open another tab."	browser_new_tab()
"Close this tab."	browser_close_tab()
"Switch to the next tab."	browser_next_tab()
"Switch to the previous tab."	browser_previous_tab()
"Go back."	browser_back()
"Go forward."	browser_forward()
"Refresh this page."	browser_refresh()
"Reload the current page."	browser_refresh()
"Open my bookmarks."	browser_bookmarks()
"Show my browsing history."	browser_history()
"Open downloads in the browser."	browser_downloads()
"Zoom in."	browser_zoom(direction="in")
"Zoom out."	browser_zoom(direction="out")
"Reset browser zoom."	browser_zoom(direction="reset")
7.2 Search and navigation to websites
Spoken input	Suggested action
"Go to GitHub."	browser_navigate(url="https://github.com")
"Open Google."	browser_navigate(url="https://www.google.com")
"Open the Python website."	browser_navigate(target="python.org")
"Search Google for Python tutorials."	browser_search(query="Python tutorials")
"Search for Rust programming examples."	browser_search(query="Rust programming examples")
"Look up the latest version of Node.js."	browser_search(query="latest version of Node.js")
"Search for my project documentation."	browser_search(query="my project documentation")
"Copy the current page URL."	browser_copy_url()
Browser safety and context
Distinguish browser navigation from opening a local application.
Resolve website names to trusted destinations.
Treat unknown or malformed URLs carefully.
Do not submit forms, purchase items, or transmit sensitive information merely because a page is open.
If multiple browser windows are open, use the active-window context or ask the user to specify one.
8. Terminal and Developer Commands
8.1 Opening development tools
Spoken input	Suggested action
"Open the terminal."	open_app(app="terminal")
"Open a terminal in the project folder."	open_terminal(cwd="project_folder")
"Open VS Code."	open_app(app="vscode")
"Open the current project in VS Code."	open_app(app="vscode", path="current_project")
"Open the current folder in the file manager."	open_folder(path="current_folder")
8.2 Common development commands
Spoken input	Suggested action
"Run Python."	terminal_run(command="python")
"Run Python app dot py."	terminal_run(command="python app.py")
"Run npm install."	terminal_run(command="npm install")
"Run npm test."	terminal_run(command="npm test")
"Run npm run build."	terminal_run(command="npm run build")
"Run Git status."	terminal_run(command="git status")
"Run Git diff."	terminal_run(command="git diff")
"Run the test suite."	terminal_run(command="configured_test_command")
"Start the development server."	start_dev_server()
"Stop the running process."	terminal_stop()
"Run the linter."	run_linter()
"Format the project."	format_project()

These command strings are examples, not an instruction to permit arbitrary shell execution.

Terminal execution policy
Recognize the intended command.
Parse the command and its arguments.
Check the executable against the application's security policy.
Validate the working directory.
Require confirmation for risky or destructive operations.
Execute in the appropriate environment.
Return the real exit status and output summary.

Do not infer missing shell arguments or silently turn dictated prose into executable shell commands.

9. File and Folder Management
Spoken input	Suggested action
"Open my downloads folder."	open_folder(path="downloads")
"Open my documents folder."	open_folder(path="documents")
"Open the desktop folder."	open_folder(path="desktop")
"Find the file named report dot pdf."	search_files(name="report.pdf")
"Search for Python files."	search_files(extension=".py")
"Find all PDF files."	search_files(extension=".pdf")
"Create a folder named screenshots."	create_folder(name="screenshots")
"Create a folder named project assets."	create_folder(name="project assets")
"Rename this file to final report dot docx."	rename_file(name="final report.docx")
"Copy the selected file."	copy_file()
"Move the selected file to downloads."	move_file(destination="downloads")
"Open the most recent PDF."	open_recent_file(type="pdf")
"Show files modified today."	search_files(modified="today")
"Show large files in downloads."	search_files(location="downloads", size="large")
"Move the selected file to documents."	move_file(destination="documents")
"Empty the recycle bin."	empty_recycle_bin()
File-management validation
Confirm the correct file or folder is selected.
Resolve relative paths against a known working directory.
Avoid guessing when multiple files have the same name.
Check whether the destination exists and is writable.
Confirm destructive operations.
Do not delete or overwrite data without the required safeguards.
10. Keyboard and Text Editing

Keyboard shortcuts vary by platform and application. Use a platform-aware shortcut mapping.

Spoken input	Suggested action
"Select all."	keyboard_shortcut(keys="SELECT_ALL")
"Copy that."	keyboard_shortcut(keys="COPY")
"Cut the selected text."	keyboard_shortcut(keys="CUT")
"Paste."	keyboard_shortcut(keys="PASTE")
"Undo that."	keyboard_shortcut(keys="UNDO")
"Redo."	keyboard_shortcut(keys="REDO")
"Save the document."	keyboard_shortcut(keys="SAVE")
"Save as."	keyboard_shortcut(keys="SAVE_AS")
"Find the word invoice."	find_text(query="invoice")
"Replace hello with goodbye."	replace_text(old="hello", new="goodbye")
"Delete the selected text."	delete_selection()
"Move the cursor to the beginning of the line."	move_cursor(position="line_start")
"Move the cursor to the end of the line."	move_cursor(position="line_end")
"Select the current paragraph."	select_paragraph()
"Capitalize the selected text."	transform_selection(style="uppercase")
"Make the selected text lowercase."	transform_selection(style="lowercase")
"Capitalize each word."	transform_selection(style="title_case")
"Bold the selected text."	format_selection(style="bold")
"Italicize the selected text."	format_selection(style="italic")
Text-editing considerations
Check whether the target application supports the action.
Confirm that the correct field or editor has focus.
Do not delete text if the selection is unexpectedly empty or broader than intended.
Distinguish editing commands from requests to dictate those same words.
11. Clipboard Commands
Spoken input	Suggested action
"Copy the selected text."	clipboard_copy()
"Paste the clipboard."	clipboard_paste()
"Clear the clipboard."	clipboard_clear()
"Copy the current page URL."	browser_copy_url()
"Copy the file path."	copy_file_path()
"Paste without formatting."	clipboard_paste_plain_text()
"Replace the clipboard with the selected text."	clipboard_copy_selection()

Clipboard contents can be sensitive. Avoid reading, logging, transmitting, or exposing clipboard contents unless necessary for the requested operation and permitted by the application.

12. Dictation and Text Transformation

These examples should normally follow the dictation path when the user is asking to write or transform text rather than operate the system.

Spoken input	Expected intent	Expected behavior
"The file is in downloads."	Dictation	Polish and inject
"I think we should ship this tomorrow."	Dictation	Polish and inject
"Let me check the code in the repository."	Dictation	Polish and inject
"What is the capital of France?"	Dictation	Preserve the question
"Please write a short update for the team."	Context-dependent	Dictation or a dedicated writing workflow
"Make this sentence more professional."	Context-dependent	Transform the selected text or dictate the request
"Capitalize the selected text."	Command	Transform selection
"Write the words open terminal."	Dictation	Insert the literal words
"Type hello world."	Command or dictation mode	Insert the requested literal text
"Insert a new paragraph."	Command	Insert a paragraph
"Delete the previous sentence."	Command	Edit the current text
"Add a comma after the word however."	Command	Apply a text edit
Text preservation tests

Test that the pipeline correctly preserves:

Application names and product names.
File names, file extensions, and paths.
URLs and email addresses.
Code identifiers and programming syntax.
Numbers, dates, times, and percentages.
Quoted text.
Emojis and protected AutoText placeholders.
Text that must not be rewritten by the polishing model.

A polishing model should not rewrite code or other protected content merely to make it sound more natural.

13. Search Commands
Spoken input	Suggested action
"Search for Python tutorials."	search(query="Python tutorials")
"Find the latest project report."	search_files(name="project report")
"Search the web for Rust documentation."	browser_search(query="Rust documentation")
"Find the word invoice in this document."	find_text(query="invoice")
"Find all PDF files in downloads."	search_files(location="downloads", extension=".pdf")
"Search for files modified yesterday."	search_files(modified="yesterday")
"Find the most recent screenshot."	search_files(type="screenshot", sort="newest")

The search target should be explicit when the same phrase could refer to local files, browser content, or a web search.

14. Multi-Action Commands

Multi-action commands should be represented as ordered action plans. Each step should have its own validated arguments and execution result.

Spoken input	Expected sequence
"Open Brave and go to GitHub."	Open browser, then navigate
"Open the terminal and run Python."	Open terminal, then run Python
"Open VS Code, then open the project folder."	Launch editor, then open project
"Turn the volume down and mute the microphone."	Lower volume, then mute microphone
"Take a screenshot and open the screenshots folder."	Capture screenshot, then open folder
"Open a new browser tab and search for Rust tutorials."	New tab, then search
"Save the document and close the window."	Save, then close
"Open downloads, then find the latest PDF."	Open folder, then search
"Launch calculator and set the volume to 20 percent."	Launch calculator, then set volume
"Open the terminal, run npm test, then show the results."	Open terminal, run tests, then display results
"Open Brave, go to GitHub, and open a new tab."	Open browser, navigate, then open tab
"Mute the speaker, lower brightness to 30 percent, and open settings."	Execute three ordered actions
Multi-action rules
Preserve the order implied by the user's words.
Validate each action independently.
Stop or ask for clarification when a required target is missing.
Do not claim all steps succeeded if one failed.
Avoid executing later dependent actions when an earlier action fails.
Require confirmation for risky operations even when they are part of a sequence.
Do not mistake a quoted command or a dictated sentence for an executable step.
15. Ambiguous Inputs

These examples are especially important for evaluating TinyDecide.classify_intent().

Spoken input	Expected label	Reason
"I need to open Brave tomorrow."	Dictation	Future intention
"Please write the words open terminal."	Dictation	Literal text requested
"My computer brightness is too low."	Dictation	Describes a problem
"The speaker was muted during the meeting."	Dictation	Past-tense statement
"We should increase our budget."	Dictation	Figurative use of increase
"I want to search for a better solution."	Dictation	General statement
"Can you tell me how to open a terminal?"	Dictation	Requests an explanation
"Open the terminal."	Command	Direct action
"Can you open the terminal?"	Command	Polite action request
"Mute the speaker."	Command	Direct device control
"The phrase is mute the speaker."	Dictation	Discussing a phrase
"When I say open Brave, it should launch the browser."	Dictation	Describes command behavior
"Open Brave and type hello world."	Command	Application action plus text entry
"Could you explain how to mute the speaker?"	Dictation	Requests an explanation
"Please mute the speaker now."	Command	Requests device control
"I forgot to turn the volume down."	Dictation	Reports a past event
"Turn the volume down."	Command	Direct action
"I want to close this chapter."	Dictation	Figurative meaning
"Close this window."	Command	UI action
"Write a tutorial about opening a terminal."	Dictation or writing workflow	Requests written content
Recommended ambiguity policy

Use context, confidence thresholds, and a safe fallback. A single keyword such as "open," "close," "run," "increase," or "delete" is not sufficient evidence that an utterance is a system command.

16. Negative Tests and False Positives

The following inputs should not trigger system actions merely because they contain command-like words.

Spoken input	Expected intent
"Open-minded people are easier to work with."	Dictation
"The command failed because the terminal was closed."	Dictation
"I muted the speaker yesterday."	Dictation
"We should increase our budget."	Dictation
"The next step is to launch the campaign."	Dictation
"I copied the file into downloads."	Dictation
"Can you explain what the mute command does?"	Dictation
"Write an email asking someone to open the browser."	Dictation or writing workflow
"The word delete appears in this document."	Dictation
"If I say restart, what will happen?"	Dictation
"The brightness of the image is too low."	Dictation
"Our team needs to run a better process."	Dictation
"I closed the application by mistake."	Dictation
"The phrase open browser is part of the documentation."	Dictation
"Explain how to create a folder."	Dictation
"Describe the steps to mute a microphone."	Dictation
"I will open the terminal later."	Dictation
"The application should launch automatically."	Dictation
"Please write a sentence that says restart the computer."	Dictation
"The file was moved to downloads."	Dictation

A false-positive command can be more harmful than a missed command. Track these errors separately.

17. Speech Recognition Robustness

Test natural variations in how users speak. The input to the intent classifier is the corrected transcript, but the full system should also be evaluated from audio to action.

17.1 Fillers and hesitation
Raw spoken input	Normalized example
"Um, open Brave."	"Open Brave."
"Could you, uh, lower the volume?"	"Could you lower the volume?"
"Okay, let's open the terminal."	Depends on filler-removal policy
"Can you please, you know, mute the speaker?"	"Can you please mute the speaker?"
"Uh, set brightness to thirty percent."	"Set brightness to 30 percent."

Do not remove words that change the meaning, negate an action, or indicate that the user is merely discussing a command.

17.2 Numbers and inverse text normalization
Spoken input	Expected argument
"Set volume to twenty percent."	value=20
"Set brightness to fifty percent."	value=50
"Set volume to one hundred percent."	value=100
"Set brightness to zero percent."	value=0
"Open file version two."	Preserve intended version identifier
"Run Python three."	Resolve the intended executable or version
"Open report dot PDF."	report.pdf
"Run app dot py."	app.py

Test numbers that sound similar, decimals, file extensions, dates, and version identifiers.

17.3 Accents, devices, and noise

Test speech captured using:

Different English accents and dialects.
Fast and slow speaking rates.
Quiet and noisy environments.
Different microphone types.
Headsets and built-in laptop microphones.
Background speech and television audio.
Proper nouns, application names, and technical vocabulary.

Maintain separate measurements for speech recognition errors and intent-classification errors. Diverse, representative audio samples are important for speech-system evaluation. See Microsoft's speech testing and training guidance.

18. Unsupported and Incomplete Commands

These examples should not lead to fabricated arguments or unrecognized actions.

Spoken input	Expected behavior
"Open it."	Ask which application or item
"Set it to 50 percent."	Ask which setting if context is unavailable
"Move the file."	Ask for the destination
"Rename the file."	Ask for the new name
"Run the command."	Ask which command
"Open the application I used yesterday."	Resolve from supported history or clarify
"Set brightness to a very high value."	Ask for or normalize a valid value
"Open an app that I don't have installed."	Explain the application is unavailable
"Run the unknown command."	Ask what command is intended
"Do the usual thing."	Use only if a clear, configured context exists

A fallback should explain what information is missing and allow the user to restate the request naturally.

19. Safety and Confirmation

Some commands have a higher cost of failure than others.

Lower-risk examples
Open an application.
Open a folder.
Switch browser tabs.
Change volume.
Search for a file.

These can generally execute immediately when intent and target are clear.

Commands that may need confirmation
Delete a file or folder.
Empty the recycle bin.
Overwrite a document.
Shut down or restart the computer.
Run arbitrary terminal commands.
Install or uninstall software.
Change permissions.
Execute a sequence that modifies important data.
Safety tests
Spoken input	Expected behavior
"Delete the selected file."	Validate selection and apply confirmation policy
"Delete the project folder permanently."	Strong confirmation and path validation
"Run this command as administrator."	Require appropriate authorization
"Shut down the computer now."	Follow configured confirmation policy
"Remove all files from downloads."	Clarify scope and require confirmation
"Run npm test."	Validate command and working directory
"Run the command I mentioned earlier."	Resolve only if the prior command is unambiguous

Never execute a destructive command just because the intent classifier returns a high confidence score. Classification, argument validation, authorization, and execution are separate decisions.

20. Suggested Intent Taxonomy

The following taxonomy can be used as a starting point. Keep the schema consistent with the actual action registry.

Intent	Required parameters	Example
open_app	app	"Open Brave."
switch_app	app	"Switch to VS Code."
open_folder	path	"Open downloads."
set_brightness	value or direction	"Dim the screen."
set_volume	value or direction	"Set volume to 20 percent."
mute_speaker	muted	"Mute the speaker."
mute_microphone	muted	"Mute my microphone."
set_theme	theme	"Turn on dark mode."
take_screenshot	Optional target	"Take a screenshot."
lock_screen	None	"Lock my computer."
sleep_system	None	"Put the computer to sleep."
browser_new_tab	None	"Open a new tab."
browser_close_tab	None	"Close this tab."
browser_navigate	url or target	"Go to GitHub."
browser_search	query	"Search for Python tutorials."
browser_refresh	None	"Refresh the page."
terminal_run	command	"Run npm test."
terminal_stop	Optional process identifier	"Stop the running process."
search_files	At least one search criterion	"Find PDF files."
create_folder	name or path	"Create a folder named assets."
rename_file	Target and new name	"Rename this file to report."
move_file	Source and destination	"Move the file to downloads."
copy_file	Source and destination or selection context	"Copy the selected file."
keyboard_shortcut	Registered shortcut	"Select all."
find_text	query	"Find the word invoice."
replace_text	old, new, and target context	"Replace hello with goodbye."
transform_selection	style	"Make the text uppercase."
multi_action	Ordered action list	"Open Brave and go to GitHub."
dictation	Transcript	"The report is ready."
clarification	Missing or ambiguous fields	"Open it."

These action names and parameter schemas are illustrative. Treat the actual command registry as the source of truth.

21. Structured Test Dataset

Use a structured test dataset to keep expected behavior independent of the model's predictions.

21.1 Recommended record fields
Field	Purpose
id	Unique test-case identifier
text	Corrected transcript presented to the router
raw_transcript	Optional original speech recognition result
expected_intent	command, dictation, or clarification
expected_action	Expected action name or null
expected_args	Expected structured arguments
expected_path	command, dictation, or clarification
risk_level	Expected risk classification
category	Test category
notes	Explanation of the expected behavior
21.2 Starter JSON dataset

The following is a JSON array suitable for a test fixture. The action names and schemas should be adapted to your implementation.

[
  {
    "id": "dictation_001",
    "text": "The file is in downloads.",
    "expected_intent": "dictation",
    "expected_action": null,
    "expected_args": {},
    "expected_path": "dictation",
    "risk_level": "none",
    "category": "basic_dictation"
  },
  {
    "id": "command_001",
    "text": "Open Brave.",
    "expected_intent": "command",
    "expected_action": "open_app",
    "expected_args": {
      "app": "brave"
    },
    "expected_path": "command",
    "risk_level": "low",
    "category": "app_launch"
  },
  {
    "id": "command_002",
    "text": "Make the screen brighter.",
    "expected_intent": "command",
    "expected_action": "set_brightness",
    "expected_args": {
      "direction": "increase"
    },
    "expected_path": "command",
    "risk_level": "low",
    "category": "brightness"
  },
  {
    "id": "command_003",
    "text": "Mute the speaker.",
    "expected_intent": "command",
    "expected_action": "mute_speaker",
    "expected_args": {
      "muted": true
    },
    "expected_path": "command",
    "risk_level": "low",
    "category": "audio"
  },
  {
    "id": "negative_001",
    "text": "I want to open Brave tomorrow.",
    "expected_intent": "dictation",
    "expected_action": null,
    "expected_args": {},
    "expected_path": "dictation",
    "risk_level": "none",
    "category": "false_positive"
  },
  {
    "id": "negative_002",
    "text": "Please write the words open terminal.",
    "expected_intent": "dictation",
    "expected_action": null,
    "expected_args": {},
    "expected_path": "dictation",
    "risk_level": "none",
    "category": "quoted_command"
  },
  {
    "id": "clarification_001",
    "text": "Open it.",
    "expected_intent": "clarification",
    "expected_action": null,
    "expected_args": {},
    "expected_path": "clarification",
    "risk_level": "none",
    "category": "missing_target"
  },
  {
    "id": "command_004",
    "text": "Run npm test.",
    "expected_intent": "command",
    "expected_action": "terminal_run",
    "expected_args": {
      "command": "npm test"
    },
    "expected_path": "command",
    "risk_level": "medium",
    "category": "developer_command"
  }
]
21.3 JSON Lines alternative

For a .jsonl dataset, store one complete JSON object per line, without enclosing the records in an array. Keep the expected labels fixed and review them before using the data for evaluation.

Suggested file layout:

voice-command-tests/
├── README.md
├── voice-command-testing-library.md
├── data/
│   ├── intent_tests.jsonl
│   ├── extraction_tests.jsonl
│   ├── dictation_negatives.jsonl
│   ├── multi_action_tests.jsonl
│   └── safety_tests.jsonl
└── reports/
    └── evaluation_results.json
22. Evaluation Metrics

Evaluate the system in stages instead of measuring only the final execution result.

Intent classification
Overall accuracy.
Precision and recall for the command class.
Precision and recall for the dictation class.
False command rate.
False dictation rate.
Confusion matrix.
Accuracy on ambiguous and quoted-command examples.
Command extraction
Correct action-name rate.
Exact argument-match rate.
Entity-resolution accuracy.
Number and percentage normalization accuracy.
Correct multi-action ordering.
Correct missing-parameter detection.
End-to-end execution
Successful execution rate.
Correct application and target selection.
Correct text injection.
Failure detection.
Correct confirmation behavior.
Unsafe execution rate.
End-to-end latency.
Suggested release gates

Define thresholds according to the consequences of failure. For example, require very strong performance on false-positive command tests and safety tests before allowing automatic execution. Do not rely on a single overall accuracy number: a model can achieve high accuracy while still making a small number of dangerous mistakes.

23. Recommended Testing Checklist

Use this checklist during development and release validation.

Intent routing
Direct commands are classified correctly.
Ordinary dictation remains on the dictation path.
Questions asking for explanations do not execute actions.
Quoted commands are treated as literal text when appropriate.
Polite command phrasing is supported.
Missing targets trigger clarification.
Low-confidence inputs do not cause unsafe actions.
Command extraction
Application aliases resolve correctly.
File names and paths are preserved.
Numbers and percentages are normalized correctly.
Unknown applications are handled safely.
Unknown actions are rejected or clarified.
Multi-action sequences preserve order.
Each action uses the registered argument schema.
Dictation
Text polishing does not change the intended meaning.
Protected text is restored accurately.
Code and file paths remain intact.
Filler removal does not remove meaningful words.
Text injection targets the correct application.
Dictation containing command keywords does not trigger actions.
Execution and safety
The executor reports real success or failure.
Destructive operations use the required confirmation.
Arbitrary shell execution is controlled.
Missing permissions are handled correctly.
Failed multi-action sequences stop or recover appropriately.
Logs do not unnecessarily expose sensitive speech or clipboard data.
Speech recognition
Common application names are recognized.
Numbers and file extensions are handled.
Accent and microphone variations are tested.
Background noise is represented.
Raw transcript errors are tracked separately from routing errors.
24. User Quick Reference
Everyday commands

Applications

"Open Brave."
"Open VS Code."
"Open the terminal."
"Switch to the browser."

Brightness and sound

"Make the screen brighter."
"Set brightness to 50 percent."
"Turn the volume down."
"Mute the microphone."

Browser

"Open a new tab."
"Go to GitHub."
"Search for Python tutorials."
"Refresh the page."

Files

"Open downloads."
"Find the latest PDF."
"Create a folder named assets."
"Move the selected file to documents."

Development

"Run Git status."
"Run npm test."
"Open the project in VS Code."
"Start the development server."

Text editing

"Select all."
"Undo that."
"Save the document."
"Replace hello with goodbye."

Multiple actions

"Open Brave and go to GitHub."
"Open the terminal and run npm test."
"Take a screenshot, then open the screenshots folder."
Final rule

Speak naturally and state the action and target clearly. The system should handle common phrasing variations, ask questions when important details are missing, and never execute a potentially dangerous operation solely because a phrase resembles a command.

Reference
Microsoft Learn, Custom Speech testing and training guidance.
Microsoft Learn, Evaluating Voice Live agents.

The command examples in this document are a proposed testing library. They are not a guarantee that any specific action is already implemented by the application.