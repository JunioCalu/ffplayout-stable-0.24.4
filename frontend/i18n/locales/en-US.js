export default {
    ok: 'Ok',
    cancel: 'Cancel',
    socketConnected: 'Message stream connected',
    socketDisconnected: 'Message stream disconnected',
    alert: {
        wrongLogin: 'Incorrect login data!',
    },
    button: {
        login: 'Login',
        home: 'Home',
        player: 'Player',
        media: 'Media',
        message: 'Message',
        logging: 'Logging',
        channels: 'Channels',
        files: 'Files',
        download: 'Download',
        restreamer: 'Restreamer',
        livebot: 'Livebot',
        configure: 'Configure',
        logout: 'Logout',
    },
    error: {
        notFound: 'Page not found',
        serverError: 'Internal server error',
    },
    input: {
        username: 'Username',
        password: 'Password',
    },
    system: {
        cpu: 'CPU',
        cores: 'Cores',
        load: 'Load',
        memory: 'Memory',
        swap: 'Swap',
        total: 'Total',
        usage: 'Usage',
        network: 'Network',
        in: 'In',
        out: 'Out',
        storage: 'Storage',
        device: 'Device',
        size: 'Size',
        used: 'Used',
    },
    control: {
        noClip: 'No clip is playing',
        ingest: 'Live Ingest',
        start: 'Start Playout Service',
        last: 'Jump to last Clip',
        stop: 'Stop Playout Service',
        reset: 'Reset Playout State',
        restart: 'Restart Playout Service',
        next: 'Jump to next Clip',
    },
    player: {
        start: 'Start',
        file: 'File',
        play: 'Play',
        title: 'Title',
        duration: 'Duration',
        total: 'Total',
        in: 'In',
        out: 'Out',
        ad: 'Ad',
        edit: 'Edit',
        description: 'Description',
        enable_description: "Enable description",
        delete: 'Delete',
        copy: 'Copy Playlist',
        loop: 'Loop Clips in Playlist',
        remote: 'Add (remote) Source to Playlist',
        import: 'Import text/m3u file',
        generate: 'Simple and advanced playlist generator',
        reset: 'Reset Playlist',
        save: 'Save Playlist',
        deletePlaylist: 'Delete Playlist',
        unsavedProgram: 'There is a program that is not saved!',
        copyTo: 'Copy current Program to',
        addEdit: 'Add/Edit Source',
        audio: 'Audio',
        customFilter: 'Custom Filter',
        deleteFrom: 'Delete program from',
        deleteSuccess: 'Playlist deleted...',
        generateProgram: 'Generate Program',
        simple: 'Simple',
        advanced: 'Advanced',
        sorted: 'Sorted',
        shuffle: 'Shuffle',
        shift: 'Shift',
        all: 'All',
        addBlock: 'Add time block',
        infinitInfo: 'Playout runs in infinite mode. No time based information is possible.',
        generateDone: 'Generate Playlist done...',
        dateYesterday: 'Current time is before the playlist start time!',
    },
    media: {
        notExists: 'Storage not exist!',
        create: 'Create Folder',
        upload: 'Upload Files',
        delete: 'Delete',
        file: 'File',
        folder: 'Folder',
        deleteQuestion: 'Are you sure that you want to delete',
        preview: 'Preview',
        rename: 'Rename File',
        newFile: 'New filename',
        createFolder: 'Create Folder',
        foldername: 'Foldername',
        current: 'Current',
        overall: 'Overall',
        uploading: 'Uploading',
        moveError: 'Move error',
        deleteError: 'Delete error',
        folderExists: 'Folder exists already',
        folderCreate: 'Folder create done...',
        folderError: 'Folder create error',
        uploadError: 'Upload error',
        fileExists: 'File exists already!',
        recursive: 'Recursive',
    },
    message: {
        savePreset: 'Save Preset',
        newPreset: 'New Preset',
        delPreset: 'Delete Preset',
        delText: 'Are you sure that you want to delete preset',
        placeholder: 'Message',
        xAxis: 'X Axis',
        yAxis: 'Y Axis',
        showBox: 'Show Box',
        boxColor: 'Box Color',
        boxAlpha: 'Box Alpha',
        size: 'Size',
        spacing: 'Spacing',
        overallAlpha: 'Overall Alpha',
        fontColor: 'Font Color',
        fontAlpha: 'Font Alpha',
        borderWidth: 'Border Width',
        send: 'Send',
        name: 'Name',
        saveDone: 'Save Preset done!',
        saveFailed: 'Save Preset failed!',
        sendDone: 'Sending success...',
        sendFailed: 'Sending failed...',
    },
    log: {
        download: 'Download log file',
        reload: 'Reload',
    },
    advanced: {
        title: 'Advanced Configuration',
        decoder: 'Decoder',
        encoder: 'Encoder',
        filter: 'Filter',
        ingest: 'Ingest',
        updateSuccess: 'Update advanced config success!',
        updateFailed: 'Update advanced config failed!',
        warning: 'Warning! These settings are experimental and only intended for advanced users who are familiar with ffmpeg. Only change the settings here if you are sure of what you are doing! The settings can make the system unstable.',
    },
    config: {
        channel: 'Channel',
        user: 'User',
        channelConf: 'Channel Configuration',
        addChannel: 'Add new Channel',
        name: 'Name',
        previewUrl: 'Preview URL',
        extensions: 'Extra Extensions',
        save: 'Save',
        delete: 'Delete',
        updateChannelSuccess: 'Update channel config success!',
        updateChannelFailed: 'Update channel config failed!',
        errorChannelDelete: 'First channel can not be deleted!',
        deleteChannelSuccess: 'Delete channel config success!',
        deleteChannelFailed: 'Delete channel config failed!',
        playoutConf: 'Playout Configuration',
        general: 'General',
        rpcServer: 'RPC Server',
        mail: 'Email',
        logging: 'Logging',
        processing: 'Processing',
        ingest: 'Ingest',
        playlist: 'Playlist',
        storage: 'Storage',
        text: 'Text',
        task: 'Task',
        output: 'Output',
        placeholderPass: 'Password',
        help: 'Help',
        generalHelp: 'Sometimes it can happen that a file is corrupt but still playable. This can produce a streaming error for all following files. The only solution in this case is to stop ffplayout and start it again.',
        stopThreshold: 'The threshold stops ffplayout if it is asynchronous in time above this value. A number below 3 can cause unexpected errors.',
        mailHelp: `Send error messages to an email address, such as missing clips, missing or invalid playlist format, etc.. Leave the recipient blank if you don't need this.`,
        mailInterval: 'The interval refers to the number of seconds until a new email is sent; the value must be in increments of 10 and not lower then 30 seconds.',
        logHelp: 'Adjust logging behavior.',
        logDetect: 'Logs an error message if the audio line is silent for 15 seconds during the validation process.',
        logIgnore: 'Ignore strings that contain matched lines; the format is a semicolon-separated list.',
        processingHelp: 'Default processing for all clips ensures uniqueness.',
        processingLogoPath: 'The logo is used only if the path exists; the path is relative to the storage folder.',
        processingLogoScale: `Leave logo scale blank if no scaling is needed. The format is 'width:height', for example: '100:-1' for proportional scaling.`,
        processingLogoPosition: `Position is specified in the format 'x:y'`,
        processingAudioTracks: 'Specify how many audio tracks should be processed.',
        processingAudioIndex: 'Which audio line to use, -1 for all.',
        processingAudioChannels: 'Set the audio channel count, if audio has more channels than stereo.',
        processingCustomFilter: 'Add custom filters to the processing. The filter outputs must end with [c_v_out] for video filters and [c_a_out] for audio filters.',
        processingVTTEnable: 'VTT can only be used in HLS mode and only if there are *.vtt files with the same name as the video file.',
        processingVTTDummy: 'A placeholder is needed if there is no vtt file.',
        ingestHelp: `Run a server for an ingest stream. This stream will override the normal streaming until it is finished. There is only a very simple authentication mechanism, which checks if the stream name is correct.`,
        ingestCustomFilter: 'Apply a custom filter to the Ingest stream in the same way as in the Processing section.',
        playlistHelp: 'Playlist handling.',
        playlistDayStart: 'At what time the playlist should start; leave it blank if the playlist should always start at the beginning.',
        playlistLength: 'Target length of the playlist; when it is blank, the real length will not be considered.',
        playlistInfinit: 'Loop a single playlist file infinitely.',
        storageHelp: 'Storage settings, locations are relative to channel storage.',
        storageFiller: 'Use filler to play in place of a missing file or to fill the remaining time to reach a total of 24 hours. It can be a file or folder, with relative path, and will loop when necessary.',
        storageExtension: 'Specify which files to search and use.',
        storageShuffle: 'Pick files randomly (in folder mode and playlist generation).',
        textHelp: 'Overlay text in combination with libzmq for remote text manipulation.',
        textFont: 'Relative path to channel storage.',
        textFromFile: 'Extraction of text from a filename.',
        textStyle: 'Define the drawtext parameters, such as position, color, etc. Posting text over the API will override this.',
        textRegex: 'Format file names to extract a title from them.',
        taskHelp: 'Run an external program with a given media object. The media object is in JSON format and contains all the information about the current clip. The external program can be a script or a binary, but it should only run for a short time.',
        taskPath: 'Path to executable.',
        outputHelp: `The final playout encoding, set the settings according to your needs. Use 'stream' mode and adjust the 'Output Parameter' when you want to stream to an RTMP/RTSP/SRT/... server.
        In production, don't serve HLS playlists with ffplayout; use Nginx or another web server!`,
        outputParam: 'HLS segment and playlist paths are relative.',
        restartTile: 'Restart Playout',
        restartText: 'Restart ffplayout to apply changes?',
        updatePlayoutSuccess: 'Update playout config success!',
        updatePlayoutFailed: 'Update playout config failed!',
        forbiddenPlaylistPath: 'Access forbidden: Playlist folder cannot be opened.',
        noPlayoutConfig: 'No playout config found!',
        publicPath: 'Public (HLS) Path',
        playlistPath: 'Playlist Path',
        storagePath: 'Storage Path',
        sharedStorage: 'ffplayout runs inside a container, use the same storage root for all channels!',
    },
    user: {
        title: 'User Configuration',
        add: 'Add User',
        delete: 'Delete',
        name: 'Username',
        mail: 'Email',
        password: 'Password',
        newPass: 'New Password',
        confirmPass: 'Confirm Password',
        save: 'Save',
        admin: 'Admin',
        deleteNotPossible: 'Delete current user not possible!',
        deleteSuccess: 'Delete user done!',
        deleteError: 'Delete user error',
        addSuccess: 'Add user success!',
        addFailed: 'Add user failed!',
        mismatch: 'Password mismatch!',
        updateSuccess: 'Update user profile success!',
        updateFailed: 'Update user profile failed!',
    },
}
