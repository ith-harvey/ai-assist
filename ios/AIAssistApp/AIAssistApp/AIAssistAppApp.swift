//
//  AIAssistAppApp.swift
//  AIAssistApp
//
//  Created by eloqjava on 2/15/26.
//

import SwiftUI
import AIAssistClientLib

@main
struct AIAssistAppApp: App {
    // TODO: Re-enable onboarding once dev-mode server config is implemented
    // @AppStorage("ai_assist_onboarding_complete") private var onboarded = false

    var body: some Scene {
        WindowGroup {
            // TODO: Re-enable onboarding gate
            // if onboarded {
            AIAssistClientLib.MainTabView()
            // } else {
            //     AIAssistClientLib.OnboardingView()
            // }
        }
    }
}
